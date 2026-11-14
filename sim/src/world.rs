//! 世界と個体データ。
//!
//! 個体は「1 個体 = 1 構造体」ではなく、属性ごとの配列（SoA）で持つ。
//! i 番目の個体の位置は (xs[i], ys[i])、種類は kind[i]、
//! 遺伝子は genomes[i * GENOME_LEN .. (i + 1) * GENOME_LEN]。
//! 配列上の位置 i は死亡のたびに詰め直されて変わるので、個体を追いかけるときは id を使う。
//!
//! 個体は毎 tick「知覚 → 脳 → 行動」の順に動く（act）。
//!
//! エネルギーは「日射 → 植物 → 草食 → 肉食」と一方通行で流れ、どこからも湧かない。
//! 出入りはすべて Ledger（帳簿）に記録し、テストで帳尻が合うことを確かめる。

use crate::genome::{self, GENOME_LEN, Mutation, N_IN, Traits, softsign};
use crate::grid::Grid;
use crate::params::Params;
use crate::plants::PlantField;
use crate::rng::Rng;

pub const HERBIVORE: u8 = 0;
pub const CARNIVORE: u8 = 1;

/// いまの状況（観察用）
pub mod state {
    /// 特に何もない（肉食が獲物を探している）
    pub const WANDER: u8 = 0;
    /// 草を食べた
    pub const GRAZE: u8 = 1;
    /// 食べずに移動している
    pub const SEEK: u8 = 2;
    /// 天敵が見えている
    pub const FLEE: u8 = 3;
    /// 獲物が見えている
    pub const CHASE: u8 = 4;
    /// 獲物を食べた
    pub const EAT: u8 = 5;
    /// 獲物を襲ったが逃げられた
    pub const MISSED: u8 = 6;
}

/// 死因（観察用）
pub mod death {
    pub const STARVED: u8 = 1;
    pub const OLD_AGE: u8 = 2;
    pub const EATEN: u8 = 3;
}

/// 近傍探索のグリッドのセルの大きさ。細かいほど、視野の小さい個体が調べる範囲を絞れる
const GRID_CELL: f32 = 16.0;

/// 描画用バッファの 1 個体あたりの要素数: [x, y, 種類, 大きさ, 標識色, 向き]
pub const RENDER_STRIDE: usize = 6;

/// エネルギーの帳簿。f64 で積算して誤差を抑える
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    /// 開始時に世界にあった量（植物 + 個体）
    pub initial: f64,
    /// 日射から植物に入った量
    pub sunlight_in: f64,
    /// 代謝と移動で失われた量
    pub lost_metabolism: f64,
    /// 食べたもののうち吸収できずに失われた量
    pub lost_digestion: f64,
    /// 繁殖で失われた量
    pub lost_birth: f64,
    /// 寿命で死んだ個体が持っていた量（死体は今のところそのまま消える）
    pub lost_death: f64,
}

impl Ledger {
    /// 帳簿の上で、いま世界にあるはずの量
    pub fn expected_total(&self) -> f64 {
        self.initial + self.sunlight_in
            - self.lost_metabolism
            - self.lost_digestion
            - self.lost_birth
            - self.lost_death
    }
}

/// 選択中の個体の観察記録。毎 tick の詳細は全個体ぶん持つと重いので、選択中の 1 体だけ記録する
#[derive(Default, Clone, Debug)]
pub struct Watch {
    /// 選択中の個体の id（0 は選択なし）
    pub id: u32,
    /// 直前の tick の移動速度
    pub speed: f32,
    /// 直前の tick に代謝と移動で失ったエネルギー
    pub cost: f32,
    /// 直前の tick の脳の出力
    pub output: [f32; genome::N_OUT],
    /// 直前の tick に曲がった角度（ラジアン、揺れを除く。正で右）
    pub turn: f32,
    /// 直前の tick の速さ（最高速度に対する割合）
    pub throttle: f32,
    /// 直前の tick に、繁殖相手が近くにいたか（無性生殖の設定なら常に true）
    pub mate_nearby: bool,
    /// 直前の tick の繁殖相手との相性（繁殖が成功する確率）。相手がいなければ負
    pub mate_compat: f32,
    /// 死んだときの (死因, tick)
    pub death: Option<(u8, u64)>,
}

/// 狩りの結果
enum Hunt {
    Caught,
    Failed,
    /// 届かない、または襲える状態でない
    NoAttempt,
}

/// 繁殖相手の見つかり具合
#[derive(Clone, Copy)]
enum Mate {
    /// 相手がいる（配列上の位置）
    Partner(usize),
    /// 相手が要らない（無性生殖の設定）
    Asexual,
    /// 相手が近くにいない
    Missing,
}

/// 1 回の知覚で見つけた相手。(配列上の位置, dx, dy, 距離の 2 乗)
type Seen = Option<(usize, f32, f32, f32)>;

struct Senses {
    predator: Seen,
    prey: Seen,
    kin: Seen,
    /// 見えている仲間の数・向きの合計・興奮している数
    kin_count: u32,
    kin_hx: f32,
    kin_hy: f32,
    kin_alert: u32,
    /// 標識色が自分と近い仲間のうち最寄り、とその数
    similar: Seen,
    similar_count: u32,
}

struct Birth {
    x: f32,
    y: f32,
    kind: u8,
    energy: f32,
    generation: u32,
    /// births_genomes の中の開始位置
    genome_at: usize,
}

pub struct World {
    pub params: Params,
    pub width: f32,
    pub height: f32,
    pub tick: u64,

    /// 個体の通し番号。1 から始まり、生まれた順に増える
    pub id: Vec<u32>,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    /// 進行方向の単位ベクトル (hx, hy)。角度で持つと毎 tick 三角関数が要り、WASM では遅いので、ベクトルで持つ
    pub hx: Vec<f32>,
    pub hy: Vec<f32>,
    pub energy: Vec<f32>,
    pub age: Vec<u32>,
    pub kind: Vec<u8>,
    /// いまの状況（state::*）
    pub state: Vec<u8>,
    /// 初期個体が 0、その子が 1、…
    pub generation: Vec<u32>,
    pub children: Vec<u32>,
    /// 遺伝子。1 個体 GENOME_LEN 個ずつ詰めてある
    pub genomes: Vec<f32>,
    /// 遺伝子から読み出した体の形質。いまの大きさに関わる値は、成長に合わせて計算し直す
    pub traits: Vec<Traits>,
    /// 成長度（生まれたとき 0.2〜1、成熟すると 1）。1 になるまで繁殖できない
    pub growth: Vec<f32>,
    /// 次に襲えるようになるまでの残り tick 数（肉食のみ使う）
    pub cooldown: Vec<u16>,
    /// tick の始めに求めた、足元の草の量（上限に対する割合）。隠れの判定に使う
    pub cover: Vec<f32>,
    /// tick の途中で死んだ個体は false にしておき、tick の最後にまとめて取り除く
    alive: Vec<bool>,

    pub plants: PlantField,
    pub ledger: Ledger,
    pub watch: Watch,

    /// JS に渡す描画用データ。fill_render() で更新する
    pub render: Vec<f32>,

    grid: Grid,
    rng: Rng,
    births: Vec<Birth>,
    births_genomes: Vec<f32>,
    next_id: u32,
    /// これまでの交配（相手がいて繁殖を試みた回数）と、そのうち相性で失敗した回数
    pub matings: u64,
    pub failed_matings: u64,
}

impl World {
    pub fn new(seed: u64, params: Params) -> Self {
        let width = params.world_width;
        let height = params.world_height;
        let plants = PlantField::new(
            width,
            height,
            params.plant_cell,
            params.plant_capacity * params.plant_initial_ratio,
        );
        let grid = Grid::new(width, height, GRID_CELL);
        let mut world = World {
            width,
            height,
            tick: 0,
            id: Vec::new(),
            xs: Vec::new(),
            ys: Vec::new(),
            hx: Vec::new(),
            hy: Vec::new(),
            energy: Vec::new(),
            age: Vec::new(),
            kind: Vec::new(),
            state: Vec::new(),
            generation: Vec::new(),
            children: Vec::new(),
            genomes: Vec::new(),
            traits: Vec::new(),
            growth: Vec::new(),
            cooldown: Vec::new(),
            cover: Vec::new(),
            alive: Vec::new(),
            plants,
            ledger: Ledger::default(),
            watch: Watch::default(),
            render: Vec::new(),
            grid,
            rng: Rng::new(seed),
            births: Vec::new(),
            births_genomes: Vec::new(),
            next_id: 1,
            matings: 0,
            failed_matings: 0,
            params,
        };
        let p = world.params.clone();
        for (kind, count, size, speed, vision, energy) in [
            (HERBIVORE, p.herb_initial, p.herb_initial_size, p.herb_initial_speed, p.herb_initial_vision, p.herb_initial_energy),
            (CARNIVORE, p.carn_initial, p.carn_initial_size, p.carn_initial_speed, p.carn_initial_vision, p.carn_initial_energy),
        ] {
            // 仲間の入力へのランダムな配線は、種類ごとに 1 回だけ引いて全員で共有する。
            // 個体ごとに引くと初期個体どうしの遺伝的距離が大きすぎて、最初から互いに繁殖できなくなる
            let mut g = genome::initial(kind == HERBIVORE, size, speed, vision, 0.0);
            genome::add_kin_wiring(&mut g, &mut world.rng);
            for _ in 0..count as usize {
                let (x, y) = world.random_position();
                // 初期個体は 1 体ずつ違う標識色にする。系統の広がりが色で見える
                g[genome::G_HUE] = world.rng.f32();
                world.spawn(x, y, kind, energy, 0, &g);
            }
        }
        world.ledger.initial = world.total_energy();
        world
    }

    fn random_position(&mut self) -> (f32, f32) {
        (self.rng.range(0.0, self.width), self.rng.range(0.0, self.height))
    }

    /// 成長しきった大人として加える
    fn spawn(&mut self, x: f32, y: f32, kind: u8, energy: f32, generation: u32, genome: &[f32]) {
        let traits = genome::traits(genome, self.params.max_turn);
        self.spawn_grown(x, y, kind, energy, generation, genome, traits, 1.0);
    }

    /// 成長度 growth の個体として加える。traits は genome から読み出したもの
    #[allow(clippy::too_many_arguments)]
    fn spawn_grown(
        &mut self,
        x: f32,
        y: f32,
        kind: u8,
        energy: f32,
        generation: u32,
        genome: &[f32],
        mut traits: Traits,
        growth: f32,
    ) {
        traits.grow_to(growth, self.params.max_turn);
        self.id.push(self.next_id);
        self.next_id += 1;
        self.xs.push(x);
        self.ys.push(y);
        let (sin, cos) = self.rng.range(0.0, std::f32::consts::TAU).sin_cos();
        self.hx.push(cos);
        self.hy.push(sin);
        self.energy.push(energy);
        self.age.push(0);
        self.kind.push(kind);
        self.state.push(state::WANDER);
        self.generation.push(generation);
        self.children.push(0);
        self.genomes.extend_from_slice(genome);
        self.traits.push(traits);
        self.growth.push(growth);
        self.cooldown.push(0);
        self.cover.push(0.0);
        self.alive.push(true);
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    pub fn count_kind(&self, kind: u8) -> usize {
        self.kind.iter().filter(|&&k| k == kind).count()
    }

    pub fn genome(&self, i: usize) -> &[f32] {
        &self.genomes[i * GENOME_LEN..(i + 1) * GENOME_LEN]
    }

    /// 世界にあるエネルギーの実際の合計（植物 + 個体）
    pub fn total_energy(&self) -> f64 {
        self.plants.total() + self.energy.iter().map(|&e| e as f64).sum::<f64>()
    }

    /// id の個体の配列上の位置。いなければ None
    pub fn index_of(&self, id: u32) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.id.iter().position(|&v| v == id)
    }

    /// (x, y) から radius 以内で最も近い個体の id。いなければ 0
    pub fn id_near(&self, x: f32, y: f32, radius: f32) -> u32 {
        self.id_near_kind(x, y, radius, None)
    }

    /// (x, y) から radius 以内で最も近い、種類 kind の個体の id（kind が None なら種類を問わない）。いなければ 0
    pub fn id_near_kind(&self, x: f32, y: f32, radius: f32, kind: Option<u8>) -> u32 {
        let mut best = radius * radius;
        let mut found = 0;
        for i in 0..self.count() {
            if kind.is_some_and(|k| self.kind[i] != k) {
                continue;
            }
            let dx = torus_delta(self.xs[i] - x, self.width);
            let dy = torus_delta(self.ys[i] - y, self.height);
            let d2 = dx * dx + dy * dy;
            if d2 < best {
                best = d2;
                found = self.id[i];
            }
        }
        found
    }

    /// 個体を選択して観察を始める（0 で選択解除）
    pub fn select(&mut self, id: u32) {
        self.watch = Watch { id, ..Watch::default() };
    }

    // ---- 種類ごとに値が変わるパラメータ（大きさによる補正込み） ----

    fn base_repro_threshold(&self, kind: u8) -> f32 {
        if kind == HERBIVORE { self.params.herb_repro_threshold } else { self.params.carn_repro_threshold }
    }

    /// 繁殖に必要なエネルギー（大人の大きさに比例）
    pub fn repro_threshold(&self, i: usize) -> f32 {
        self.base_repro_threshold(self.kind[i]) * self.traits[i].adult_size
    }

    /// 蓄えられるエネルギーの上限（いまの大きさに比例。幼体は小さい）
    pub fn energy_cap(&self, i: usize) -> f32 {
        self.base_repro_threshold(self.kind[i]) * self.traits[i].size * self.params.energy_cap_ratio
    }

    /// 1 tick 進める
    pub fn step(&mut self) {
        self.ledger.sunlight_in += self
            .plants
            .grow(self.params.sunlight, self.params.plant_capacity);

        let cap = self.params.plant_capacity;
        for i in 0..self.count() {
            self.cover[i] = self.plants.ratio_at(self.xs[i], self.ys[i], cap);
        }
        self.grid.build(&self.xs, &self.ys, &self.kind, &self.cover);

        // セル順に回す（近くの個体を続けて処理するとキャッシュに乗って速い）
        for k in 0..self.count() {
            let i = self.grid.entry(k).0;
            if !self.alive[i] {
                continue; // この tick ですでに食べられた
            }
            let (speed, wants_child, mate) = self.act(i);
            self.move_agent(i, speed);
            self.live(i, speed, wants_child, mate);
        }

        self.apply_births();
        self.remove_dead();
        self.tick += 1;
    }

    /// 視野の中で最も近い天敵・獲物・仲間を探す。
    /// 相手のいるマスの草が多いほど、その相手が見える距離は縮む（隠れ）
    fn sense(&self, i: usize) -> Senses {
        let (x, y) = (self.xs[i], self.ys[i]);
        let (w, h) = (self.width, self.height);
        let vision = self.traits[i].vision;
        let hide = self.params.cover_strength;
        let my_kind = self.kind[i];
        // [天敵, 獲物, 仲間]
        let mut best: [Seen; 3] = [None, None, None];
        let mut best_d2 = [f32::MAX; 3];
        let (mut kin_count, mut kin_hx, mut kin_hy, mut kin_alert) = (0u32, 0.0f32, 0.0f32, 0u32);
        let (mut similar, mut similar_d2, mut similar_count): (Seen, f32, u32) = (None, f32::MAX, 0);
        let my_hue = self.traits[i].hue;
        let hue_similar = self.params.hue_similar;
        let vision2 = vision * vision;
        // 相手の種類と足元の草はグリッドがセル順に並べた写しから受け取る（離れたメモリを読まずに済む）。
        // 安い判定から順に行い、遠い相手はそこで打ち切る
        self.grid.for_each_within(x, y, vision, |j, xj, yj, kind_j, cover_j| {
            let dx = torus_delta(xj - x, w);
            let dy = torus_delta(yj - y, h);
            let d2 = dx * dx + dy * dy;
            if d2 >= vision2 {
                return;
            }
            let r = vision * (1.0 - hide * cover_j);
            if d2 < r * r && j != i && self.alive[j] {
                let rel = if kind_j == my_kind {
                    2
                } else if my_kind == HERBIVORE {
                    0
                } else {
                    1
                };
                if d2 < best_d2[rel] {
                    best_d2[rel] = d2;
                    best[rel] = Some((j, dx, dy, d2));
                }
                if rel == 2 {
                    kin_count += 1;
                    kin_hx += self.hx[j];
                    kin_hy += self.hy[j];
                    // 草食なら天敵が、肉食なら獲物が見えている仲間
                    let s = self.state[j];
                    kin_alert += (s == state::FLEE || s == state::CHASE) as u32;
                    if genome::hue_distance(my_hue, self.traits[j].hue) <= hue_similar {
                        similar_count += 1;
                        if d2 < similar_d2 {
                            similar_d2 = d2;
                            similar = Some((j, dx, dy, d2));
                        }
                    }
                }
            }
        });
        Senses {
            predator: best[0],
            prey: best[1],
            kin: best[2],
            kin_count,
            kin_hx,
            kin_hy,
            kin_alert,
            similar,
            similar_count,
        }
    }

    /// 知覚 → 脳 → 行動。戻り値は (この tick の移動速度, 繁殖したいか, 繁殖相手)
    fn act(&mut self, i: usize) -> (f32, bool, Mate) {
        let senses = self.sense(i);
        let t = self.traits[i];
        let (x, y) = (self.xs[i], self.ys[i]);
        let (hx, hy) = (self.hx[i], self.hy[i]);
        // 世界の向き (dx, dy) を、自分の向きを基準にした (前方, 右方) に直す。右方は (−hy, hx)
        let to_local = |dx: f32, dy: f32| (dx * hx + dy * hy, -dx * hy + dy * hx);

        let mut input = [0.0f32; N_IN];
        input[genome::IN_ENERGY] = self.energy[i] / self.repro_threshold(i);
        input[genome::IN_PLANT_HERE] = self.cover[i];
        if let Some((dc, dr)) = self.plants.richer_direction(x, y) {
            let len = (dc * dc + dr * dr).sqrt();
            let (f, r) = to_local(dc / len, dr / len);
            input[genome::IN_GRAD_FWD] = f;
            input[genome::IN_GRAD_RIGHT] = r;
        }
        for (seen, at) in [
            (senses.predator, genome::IN_PRED_FWD),
            (senses.prey, genome::IN_PREY_FWD),
            (senses.kin, genome::IN_KIN_FWD),
        ] {
            if let Some((_, dx, dy, d2)) = seen {
                let d = d2.sqrt().max(1e-3);
                let (f, r) = to_local(dx / d, dy / d);
                input[at] = f;
                input[at + 1] = r;
                // 見えていれば 0.5〜1（近いほど大きい）、見えていなければ 0。
                // 「見えたか」と「どれだけ近いか」の両方をこの 1 つで伝える
                input[at + 2] = 1.0 - 0.5 * (d / t.vision).min(1.0);
            }
        }

        if senses.kin_count > 0 {
            let n = senses.kin_count as f32;
            input[genome::IN_KIN_COUNT] = softsign(n / 4.0);
            let (f, r) = to_local(senses.kin_hx / n, senses.kin_hy / n);
            input[genome::IN_KIN_HEAD_FWD] = f;
            input[genome::IN_KIN_HEAD_RIGHT] = r;
            input[genome::IN_KIN_ALERT] = senses.kin_alert as f32 / n;
            input[genome::IN_SIM_RATIO] = senses.similar_count as f32 / n;
        }
        if let Some((_, dx, dy, d2)) = senses.similar {
            let d = d2.sqrt().max(1e-3);
            let (f, r) = to_local(dx / d, dy / d);
            input[genome::IN_SIM_FWD] = f;
            input[genome::IN_SIM_RIGHT] = r;
        }

        let out = genome::think(self.genome(i), &input);
        let is_watched = self.id[i] == self.watch.id;
        if is_watched {
            self.watch.output = out;
        }

        // 行きたい方向へ曲がる。1 tick に曲がれる角度には上限があり、大きいほど小回りが利かない
        let (lf, lr) = steer(out[genome::OUT_DIR_FWD], out[genome::OUT_DIR_RIGHT], t.turn_cos, t.turn_sin);
        // 自分基準の (前方, 右方) を世界の向きに戻し、小さな揺れを加える
        let (mut nx, mut ny) = (lf * hx - lr * hy, lf * hy + lr * hx);
        let noise = self.rng.range(-self.params.turn_noise, self.params.turn_noise);
        // 小さな角度 ε の回転は (x − εy, y + εx) で近似できる。ずれた長さは直後に 1 に戻す
        (nx, ny) = (nx - noise * ny, ny + noise * nx);
        let len = (nx * nx + ny * ny).sqrt();
        self.hx[i] = nx / len;
        self.hy[i] = ny / len;
        let throttle = 0.5 + 0.5 * softsign(out[genome::OUT_THROTTLE]);
        // 幼体は遅い
        let jv = self.params.juvenile_speed;
        let speed = t.max_speed * throttle * (jv + (1.0 - jv) * self.growth[i]);
        if is_watched {
            self.watch.turn = lr.atan2(lf);
            self.watch.throttle = throttle;
        }
        let wants_eat = out[genome::OUT_EAT] > 0.0;
        let wants_child = out[genome::OUT_REPRODUCE] > 0.0;
        let mate = self.find_mate(i, &senses);
        if is_watched {
            self.watch.mate_nearby = !matches!(mate, Mate::Missing);
            self.watch.mate_compat = match mate {
                Mate::Partner(j) => self.mating_chance(i, j),
                Mate::Asexual => 1.0,
                Mate::Missing => -1.0,
            };
        }

        if self.kind[i] == HERBIVORE {
            let ate = wants_eat && self.graze(i);
            self.state[i] = if senses.predator.is_some() {
                state::FLEE
            } else if ate {
                state::GRAZE
            } else {
                state::SEEK
            };
            (speed, wants_child, mate)
        } else {
            if wants_eat && let Some((j, _, _, d2)) = senses.prey {
                match self.try_hunt(i, j, d2) {
                    Hunt::Caught => {
                        self.state[i] = state::EAT;
                        return (0.0, wants_child, mate); // 食べた tick は動かない
                    }
                    Hunt::Failed => {
                        self.state[i] = state::MISSED;
                        return (speed, wants_child, mate);
                    }
                    Hunt::NoAttempt => {}
                }
            }
            self.state[i] = if senses.prey.is_some() { state::CHASE } else { state::WANDER };
            (speed, wants_child, mate)
        }
    }

    /// 繁殖相手を探す。繁殖相手を探せる距離以内に見えている最寄りの仲間が相手になる。
    /// その距離が 0 以下なら無性生殖（相手は要らない）
    fn find_mate(&self, i: usize, senses: &Senses) -> Mate {
        let range = if self.kind[i] == HERBIVORE { self.params.herb_mate_range } else { self.params.carn_mate_range };
        if range <= 0.0 {
            return Mate::Asexual;
        }
        match senses.kin {
            Some((j, _, _, d2)) if d2 <= range * range => Mate::Partner(j),
            _ => Mate::Missing,
        }
    }

    /// いまいるマスの草を食べる。蓄えの上限を超える分は食べない。食べたら true
    fn graze(&mut self, i: usize) -> bool {
        let room = self.energy_cap(i) - self.energy[i];
        if room <= 0.0 {
            return false;
        }
        let eff = self.params.herb_efficiency;
        let c = self.plants.cell_index(self.xs[i], self.ys[i]);
        let take = (self.params.herb_bite * self.traits[i].size)
            .min(self.plants.biomass[c])
            .min(room / eff);
        if take <= 0.0 {
            return false;
        }
        self.plants.biomass[c] -= take;
        let gain = take * eff;
        self.energy[i] += gain;
        self.ledger.lost_digestion += (take - gain) as f64;
        true
    }

    /// 獲物 j を襲う。届く距離にいて、大きすぎなければ食べる。食べたら true
    fn try_hunt(&mut self, i: usize, j: usize, d2: f32) -> Hunt {
        let range = self.params.carn_attack_range * self.traits[i].size;
        if d2 >= range * range || self.cooldown[i] > 0 {
            return Hunt::NoAttempt;
        }
        // 襲いかかる。成功・失敗にかかわらずエネルギーを使い、しばらく次を襲えない
        let cost = self.params.attack_cost.min(self.energy[i]);
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.cooldown[i] = self.params.attack_cooldown as u16;
        if self.rng.f32() >= self.hunt_chance(i, j) {
            return Hunt::Failed; // 逃げられた
        }
        let prey = self.energy[j];
        let room = (self.energy_cap(i) - self.energy[i]).max(0.0);
        // 食べきれない分は捨てる（熱として失われる）
        let gain = (prey * self.params.carn_efficiency).min(room);
        self.energy[i] += gain;
        self.ledger.lost_digestion += (prey - gain) as f64;
        self.energy[j] = 0.0;
        self.die(j, death::EATEN);
        Hunt::Caught
    }

    /// 肉食 i が獲物 j を襲って成功する確率。
    /// 獲物が自分より大きいほど下がり、獲物の近くに仲間（同じ種類）が多いほど下がる（数で守る）
    pub fn hunt_chance(&self, i: usize, j: usize) -> f32 {
        let p = &self.params;
        let ratio = self.traits[j].size / self.traits[i].size;
        let by_size = 1.0 - ((ratio - p.hunt_easy_ratio) / (p.hunt_impossible_ratio - p.hunt_easy_ratio)).clamp(0.0, 1.0);
        if by_size <= 0.0 {
            return 0.0;
        }
        let (x, y) = (self.xs[j], self.ys[j]);
        let prey_kind = self.kind[j];
        let r2 = p.guard_radius * p.guard_radius;
        let mut guards = 0u32;
        self.grid.for_each_within(x, y, p.guard_radius, |k, xk, yk, kind_k, _| {
            if k != j && kind_k == prey_kind && self.alive[k] {
                let dx = torus_delta(xk - x, self.width);
                let dy = torus_delta(yk - y, self.height);
                guards += (dx * dx + dy * dy < r2) as u32;
            }
        });
        p.hunt_success * by_size / (1.0 + p.guard_strength * guards as f32)
    }

    fn move_agent(&mut self, i: usize, speed: f32) {
        if speed == 0.0 {
            return;
        }
        self.xs[i] = wrap(self.xs[i] + self.hx[i] * speed, self.width);
        self.ys[i] = wrap(self.ys[i] + self.hy[i] * speed, self.height);
    }

    /// 個体 i を死亡扱いにする。エネルギーは呼び出し側で 0 にしておくこと
    fn die(&mut self, i: usize, cause: u8) {
        self.alive[i] = false;
        if self.id[i] == self.watch.id {
            self.watch.death = Some((cause, self.tick));
        }
    }

    /// エネルギー消費・加齢・死亡・繁殖
    fn live(&mut self, i: usize, speed: f32, wants_child: bool, mate: Mate) {
        let p = &self.params;
        let t = self.traits[i];
        let herb = self.kind[i] == HERBIVORE;
        let (metabolism, max_age, birth_cost) = if herb {
            (p.herb_metabolism, p.herb_max_age, p.herb_birth_cost)
        } else {
            (p.carn_metabolism, p.carn_max_age, p.carn_birth_cost)
        };

        // 代謝は大きさの 0.75 乗と視野に、移動は大きさと速度の 2 乗に比例する。
        // 持っている以上は失えない（エネルギーが負になって帳尻が狂うのを防ぐ）
        let cost = (metabolism * t.size_pow075 + p.vision_cost * t.vision + p.move_cost * t.size * speed * speed)
            .min(self.energy[i]);
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.age[i] += 1;
        self.cooldown[i] = self.cooldown[i].saturating_sub(1);
        self.grow(i, if herb { p.herb_maturity } else { p.carn_maturity });
        if self.id[i] == self.watch.id {
            self.watch.speed = speed;
            self.watch.cost = cost;
        }

        if self.energy[i] <= 0.0 {
            self.energy[i] = 0.0;
            self.die(i, death::STARVED);
            return;
        }
        if self.age[i] as f32 > max_age * t.adult_sqrt {
            self.ledger.lost_death += self.energy[i] as f64;
            self.energy[i] = 0.0;
            self.die(i, death::OLD_AGE);
            return;
        }
        // 成熟するまで繁殖できない
        if wants_child && self.growth[i] >= 1.0 && self.energy[i] >= self.repro_threshold(i) {
            match mate {
                Mate::Partner(j) => {
                    self.matings += 1;
                    if self.rng.f32() < self.mating_chance(i, j) {
                        self.reproduce(i, birth_cost * t.adult_size, Some(j));
                    } else {
                        // 相性が悪く失敗した。繁殖のコストは払う（払わないと、相手を選ぶ利益が生まれない）
                        self.failed_matings += 1;
                        let cost = (birth_cost * t.adult_size).min(self.energy[i]);
                        self.energy[i] -= cost;
                        self.ledger.lost_birth += cost as f64;
                    }
                }
                Mate::Asexual => self.reproduce(i, birth_cost * t.adult_size, None),
                Mate::Missing => {} // 相手がいなければ繁殖できない
            }
        }
    }

    /// i と j が繁殖に成功する確率（遺伝的に遠いほど下がる）
    pub fn mating_chance(&self, i: usize, j: usize) -> f32 {
        let p = &self.params;
        let d = genome::genetic_distance(self.genome(i), self.genome(j), p.mutation_trait_sigma, p.mutation_weight_sigma);
        genome::compatibility(d, p.compat_full_distance, p.compat_zero_distance)
    }

    /// 成長させる。maturity は成長度 0 から 1 になるまでの tick 数
    fn grow(&mut self, i: usize, maturity: f32) {
        if self.growth[i] >= 1.0 {
            return;
        }
        self.growth[i] = (self.growth[i] + 1.0 / maturity).min(1.0);
        // いまの大きさから決まる値（0.75 乗など）の計算は重いので、8 tick ごとと成熟したときだけ行う
        if self.age[i] % 8 == 0 || self.growth[i] >= 1.0 {
            let g = self.growth[i];
            self.traits[i].grow_to(g, self.params.max_turn);
        }
    }

    /// 繁殖コストを引いた残りのうち、子への投資の割合を子に渡す。
    /// エネルギーを払うのは繁殖を決めた側（i）だけで、相手（mate）は遺伝子を渡すだけ
    fn reproduce(&mut self, i: usize, birth_cost: f32, mate: Option<usize>) {
        let before = self.energy[i];
        let available = before - birth_cost;
        let child = available * self.traits[i].invest;
        let parent = available - child;
        self.energy[i] = parent;
        self.ledger.lost_birth += (before - parent - child) as f64;
        self.children[i] += 1;

        let genome_at = self.births_genomes.len();
        let mine = &self.genomes[i * GENOME_LEN..(i + 1) * GENOME_LEN];
        match mate {
            Some(j) => {
                let theirs = &self.genomes[j * GENOME_LEN..(j + 1) * GENOME_LEN];
                genome::crossover(mine, theirs, &mut self.rng, &mut self.births_genomes);
            }
            None => self.births_genomes.extend_from_slice(mine),
        }
        let m = Mutation {
            rate: self.params.mutation_rate,
            trait_sigma: self.params.mutation_trait_sigma,
            weight_sigma: self.params.mutation_weight_sigma,
            hue_sigma: self.params.mutation_hue_sigma,
        };
        genome::mutate(&mut self.births_genomes[genome_at..], &m, &mut self.rng);

        let spread = self.params.birth_spread;
        let x = self.xs[i] + self.rng.range(-spread, spread);
        let y = self.ys[i] + self.rng.range(-spread, spread);
        self.births.push(Birth {
            x,
            y,
            kind: self.kind[i],
            energy: child,
            generation: self.generation[i] + 1,
            genome_at,
        });
    }

    fn apply_births(&mut self) {
        let births = std::mem::take(&mut self.births);
        let genomes = std::mem::take(&mut self.births_genomes);
        for b in &births {
            let g = &genomes[b.genome_at..b.genome_at + GENOME_LEN];
            let traits = genome::traits(g, self.params.max_turn);
            // 生まれたときの成長度は、受け取ったエネルギーが大人の繁殖に必要な量の何割か。
            // 子への投資が多いほど大きく生まれる
            let adult_need = self.base_repro_threshold(b.kind) * traits.adult_size;
            let growth = (b.energy / adult_need).clamp(self.params.min_birth_growth, 1.0);
            let (x, y) = (wrap(b.x, self.width), wrap(b.y, self.height));
            self.spawn_grown(x, y, b.kind, b.energy, b.generation, g, traits, growth);
        }
        self.births = births;
        self.births.clear();
        self.births_genomes = genomes;
        self.births_genomes.clear();
    }

    /// 死んだ個体を取り除く。順番は保つ（決定論のため）
    fn remove_dead(&mut self) {
        // 後ろから見ていき、死んだ個体の場所に末尾の個体を移す（swap_remove）。
        // 順番を保って詰めると、遺伝子（1 体 GENOME_LEN 個）を毎 tick ほぼ全員分コピーすることになり重い。
        // この方法でも同じ入力なら同じ結果になる（決定論は保たれる）
        for i in (0..self.count()).rev() {
            if self.alive[i] {
                continue;
            }
            self.id.swap_remove(i);
            self.xs.swap_remove(i);
            self.ys.swap_remove(i);
            self.hx.swap_remove(i);
            self.hy.swap_remove(i);
            self.energy.swap_remove(i);
            self.age.swap_remove(i);
            self.kind.swap_remove(i);
            self.state.swap_remove(i);
            self.generation.swap_remove(i);
            self.children.swap_remove(i);
            self.traits.swap_remove(i);
            self.growth.swap_remove(i);
            self.cooldown.swap_remove(i);
            self.cover.swap_remove(i);
            self.alive.swap_remove(i);
            let last = self.genomes.len() - GENOME_LEN;
            if i * GENOME_LEN != last {
                self.genomes.copy_within(last.., i * GENOME_LEN);
            }
            self.genomes.truncate(last);
        }
    }

    pub fn fill_render(&mut self) {
        self.render.resize(self.count() * RENDER_STRIDE, 0.0);
        for i in 0..self.count() {
            let o = i * RENDER_STRIDE;
            self.render[o] = self.xs[i];
            self.render[o + 1] = self.ys[i];
            self.render[o + 2] = self.kind[i] as f32;
            self.render[o + 3] = self.traits[i].size;
            self.render[o + 4] = self.traits[i].hue;
            // 向き（ラジアン）。描画で回すのは肉食（三角形）だけなので、計算も肉食だけ
            self.render[o + 5] = if self.kind[i] == CARNIVORE { self.hy[i].atan2(self.hx[i]) } else { 0.0 };
        }
    }

    /// 種類ごとの形質の平均。[個体数, 大きさ, 最高速度, 視野, 子への投資, 世代]
    pub fn trait_means(&self, kind: u8) -> [f64; 6] {
        let mut sum = [0.0f64; 6];
        for i in 0..self.count() {
            if self.kind[i] != kind {
                continue;
            }
            let t = &self.traits[i];
            sum[0] += 1.0;
            sum[1] += t.adult_size as f64;
            sum[2] += t.max_speed as f64;
            sum[3] += t.vision as f64;
            sum[4] += t.invest as f64;
            sum[5] += self.generation[i] as f64;
        }
        let n = sum[0];
        if n > 0.0 {
            for v in &mut sum[1..] {
                *v /= n;
            }
        }
        sum
    }

    /// 群れの度合い。[集まり具合（Clark-Evans 比）, 向きのそろい具合, 同じ系統どうしのそろい具合,
    /// 違う系統どうしのそろい具合, 近くの仲間のうち同じ系統の割合]
    ///
    /// - 集まり具合: 最寄りの仲間までの平均距離 ÷ 同じ密度でランダムに散らばった場合の期待値（0.5 / √密度）。
    ///   1 ならランダム、1 未満なら集まっている、1 より大きければ互いに避けている
    /// - 向きのそろい具合: 自分と、半径 HERD_RADIUS 内の仲間の進行方向の cos の平均。0 ならばらばら、1 なら全員同じ向き
    ///
    /// 集まり具合だけだと「草の多い場所に別々に集まっている」のと区別できないので、向きも見る。
    /// 同じ系統（標識色の差が hue_similar 以下）どうしでだけ向きがそろっていれば、系統の群れと言える。
    /// 計算を軽くするため、HERD_SAMPLE_STEP 体に 1 体だけ調べる。
    /// グリッドを今の位置で作り直す（step の最初にも作り直すので、シミュレーションには影響しない）
    pub fn herd_stats(&mut self, kind: u8) -> [f64; 5] {
        const HERD_RADIUS: f32 = 32.0;
        const HERD_SAMPLE_STEP: usize = 4;
        /// 最寄りの仲間を探す範囲。これより遠ければこの距離として数える（疎な種類ではやや過小になる）
        const NN_LIMIT: f32 = 128.0;

        self.grid.build(&self.xs, &self.ys, &self.kind, &self.cover);
        let n_kind = self.count_kind(kind);
        if n_kind < 2 {
            return [f64::NAN; 5];
        }
        let (w, h) = (self.width, self.height);
        let (mut nn_sum, mut nn_n) = (0.0f64, 0usize);
        let (mut align_sum, mut align_n) = (0.0f64, 0usize);
        // [同じ系統, 違う系統] それぞれの向きの cos の合計と数
        let (mut lin_sum, mut lin_n) = ([0.0f64; 2], [0usize; 2]);
        let hue_similar = self.params.hue_similar;
        for i in (0..self.count()).step_by(HERD_SAMPLE_STEP) {
            if self.kind[i] != kind {
                continue;
            }
            let (x, y) = (self.xs[i], self.ys[i]);
            let (hx, hy) = (self.hx[i], self.hy[i]);
            let my_hue = self.traits[i].hue;
            let mut best = NN_LIMIT * NN_LIMIT;
            self.grid.for_each_within(x, y, NN_LIMIT, |j, xj, yj, kind_j, _| {
                if j == i || kind_j != kind {
                    return;
                }
                let dx = torus_delta(xj - x, w);
                let dy = torus_delta(yj - y, h);
                let d2 = dx * dx + dy * dy;
                best = best.min(d2);
                if d2 < HERD_RADIUS * HERD_RADIUS {
                    let c = (hx * self.hx[j] + hy * self.hy[j]) as f64;
                    align_sum += c;
                    align_n += 1;
                    let other = (genome::hue_distance(my_hue, self.traits[j].hue) > hue_similar) as usize;
                    lin_sum[other] += c;
                    lin_n[other] += 1;
                }
            });
            nn_sum += (best as f64).sqrt();
            nn_n += 1;
        }
        let density = n_kind as f64 / (w as f64 * h as f64);
        let expected = 0.5 / density.sqrt();
        let ratio = if nn_n > 0 { nn_sum / nn_n as f64 / expected } else { f64::NAN };
        let mean = |s: f64, n: usize| if n > 0 { s / n as f64 } else { f64::NAN };
        let same_share = mean(lin_n[0] as f64, align_n);
        [ratio, mean(align_sum, align_n), mean(lin_sum[0], lin_n[0]), mean(lin_sum[1], lin_n[1]), same_share]
    }
}

/// 行きたい方向 (fwd, right)（自分基準）へ、最大旋回角までだけ向きを変えた後の向き（自分基準の単位ベクトル）。
/// turn_cos, turn_sin は最大旋回角の cos と sin
fn steer(fwd: f32, right: f32, turn_cos: f32, turn_sin: f32) -> (f32, f32) {
    let len = (fwd * fwd + right * right).sqrt();
    if len < 1e-6 {
        return (1.0, 0.0); // 行きたい方向がなければ、まっすぐ
    }
    let (f, r) = (fwd / len, right / len);
    if f >= turn_cos {
        (f, r) // 最大旋回角より内側なので、そのまま向ける
    } else if r >= 0.0 {
        (turn_cos, turn_sin) // 右へ最大まで
    } else {
        (turn_cos, -turn_sin) // 左へ最大まで
    }
}

/// 座標を [0, size) に折り返す（トーラス世界）
fn wrap(v: f32, size: f32) -> f32 {
    if v < 0.0 {
        v + size
    } else if v >= size {
        v - size
    } else {
        v
    }
}

/// トーラス上での最短の差分
pub fn torus_delta(d: f32, size: f32) -> f32 {
    if d > size * 0.5 {
        d - size
    } else if d < -size * 0.5 {
        d + size
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_params() -> Params {
        let mut p = Params::default();
        p.world_width = 640.0;
        p.world_height = 480.0;
        p.herb_initial = 600.0;
        p.carn_initial = 40.0;
        p
    }

    #[test]
    fn 同じシードなら同じ歴史になる() {
        let mut a = World::new(7, small_params());
        let mut b = World::new(7, small_params());
        for _ in 0..300 {
            a.step();
            b.step();
        }
        assert_eq!(a.xs, b.xs);
        assert_eq!(a.energy, b.energy);
        assert_eq!(a.genomes, b.genomes);
    }

    #[test]
    fn エネルギーの帳尻が合う() {
        let mut w = World::new(3, small_params());
        for _ in 0..2000 {
            w.step();
        }
        let expected = w.ledger.expected_total();
        let actual = w.total_energy();
        let rel = (expected - actual).abs() / expected.abs().max(1.0);
        assert!(
            rel < 1e-4,
            "帳簿上 {expected:.3} / 実際 {actual:.3}（相対誤差 {rel:e}）\n{:?}",
            w.ledger
        );
    }

    #[test]
    fn 座標は世界の中に収まる() {
        let mut w = World::new(9, small_params());
        for _ in 0..500 {
            w.step();
        }
        assert!(w.xs.iter().all(|&x| (0.0..640.0).contains(&x)));
        assert!(w.ys.iter().all(|&y| (0.0..480.0).contains(&y)));
    }

    #[test]
    fn 日射がなければ植物は増えない() {
        let mut p = small_params();
        p.sunlight = 0.0;
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        let mut w = World::new(1, p);
        let before = w.plants.total();
        for _ in 0..100 {
            w.step();
        }
        assert_eq!(w.plants.total(), before);
    }

    #[test]
    fn 死んだ個体のエネルギーは残らない() {
        let mut w = World::new(5, small_params());
        for _ in 0..500 {
            w.step();
            assert!(w.energy.iter().all(|&e| e > 0.0));
        }
    }

    #[test]
    fn 遺伝子と形質は個体と一緒に詰め直される() {
        let mut w = World::new(4, small_params());
        for _ in 0..400 {
            w.step();
            assert_eq!(w.genomes.len(), w.count() * GENOME_LEN);
            for i in (0..w.count()).step_by(37) {
                let t = genome::traits(w.genome(i), w.params.max_turn);
                assert_eq!(t.adult_size, w.traits[i].adult_size, "個体 {i} の遺伝子と形質がずれている");
                assert_eq!(t.hue, w.traits[i].hue);
            }
        }
    }

    #[test]
    fn 草むらにいる相手は見つけにくい() {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        let mut w = World::new(1, p);
        let g = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(130.0, 100.0, HERBIVORE, 5.0, 0, &g);
        w.params.cover_strength = 0.6;

        w.cover = vec![0.0, 0.0];
        w.grid.build(&w.xs, &w.ys, &w.kind, &w.cover);
        assert!(w.sense(0).prey.is_some(), "草がなければ 30px 先の獲物が見える");
        // 草が満杯なら視野 40 × (1 - 0.6) = 16px までしか見えない
        w.cover = vec![0.0, 1.0];
        w.grid.build(&w.xs, &w.ys, &w.kind, &w.cover);
        assert!(w.sense(0).prey.is_none(), "草むらの獲物は見えない");
    }

    #[test]
    fn idは重複せず死亡と詰め直しの後も個体についていく() {
        let mut w = World::new(11, small_params());
        let target = *w.id.last().unwrap();
        w.select(target);
        let mut last_age = 0;
        for _ in 0..200 {
            w.step();
            let mut ids = w.id.clone();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), w.count(), "id が重複している");
            match w.index_of(target) {
                Some(i) => {
                    assert_eq!(w.age[i], last_age + 1, "別の個体を指している");
                    last_age = w.age[i];
                }
                None => {
                    assert!(w.watch.death.is_some(), "死因が記録されずに消えた");
                    break;
                }
            }
        }
    }

    #[test]
    fn 相手がいなければ繁殖しない() {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        p.sunlight = 0.0;
        let mut w = World::new(1, p);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        // 1 体だけ、繁殖に十分なエネルギーを持たせる
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        for _ in 0..50 {
            w.step();
        }
        assert_eq!(w.count(), 1, "相手がいないのに繁殖した");

        // 近くに相手がいれば繁殖する
        w.spawn(w.xs[0] + 3.0, w.ys[0], HERBIVORE, 5.0, 0, &g);
        w.energy[0] = 15.0;
        w.step();
        assert!(w.count() > 2, "相手がいるのに繁殖しなかった");
    }

    fn empty_world() -> World {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        p.sunlight = 0.0;
        World::new(1, p)
    }

    #[test]
    fn 大きい獲物ほど狩りに失敗しやすい() {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        for (k, size) in [0.5, 1.0, 1.4, 2.0].iter().enumerate() {
            let g = genome::initial(true, *size, 1.0, 30.0, 0.0);
            // 獲物同士が守り合わないよう、離して置く
            w.spawn(300.0 + 80.0 * k as f32, 300.0, HERBIVORE, 5.0, 0, &g);
        }
        w.grid.build(&w.xs, &w.ys, &w.kind, &w.cover);
        let chances: Vec<f32> = (1..=4).map(|j| w.hunt_chance(0, j)).collect();
        assert_eq!(chances[0], w.params.hunt_success, "十分小さい獲物は最大の成功率");
        assert!(chances[1] < chances[0] && chances[2] < chances[1], "大きいほど下がる: {chances:?}");
        assert_eq!(chances[3], 0.0, "大きすぎる獲物は狩れない");
    }

    #[test]
    fn 仲間に囲まれた獲物は狩りにくい() {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        let herb = genome::initial(true, 0.5, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        w.spawn(105.0, 100.0, HERBIVORE, 5.0, 0, &herb);
        w.grid.build(&w.xs, &w.ys, &w.kind, &w.cover);
        let alone = w.hunt_chance(0, 1);
        for k in 0..4 {
            w.spawn(105.0 + (k as f32 - 1.5) * 4.0, 104.0, HERBIVORE, 5.0, 0, &herb);
        }
        w.grid.build(&w.xs, &w.ys, &w.kind, &w.cover);
        let guarded = w.hunt_chance(0, 1);
        assert!(guarded < alone * 0.7, "囲まれていると下がる: 単独 {alone} / 囲まれ {guarded}");
    }

    #[test]
    fn 子は小さく生まれて成熟するまで繁殖しない() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &g);
        w.step();
        assert_eq!(w.count(), 3, "親が繁殖する");
        let child = (0..w.count()).find(|&i| w.generation[i] == 1).unwrap();
        assert!(w.growth[child] < 1.0, "子は成長途中で生まれる");
        assert!(w.traits[child].size < w.traits[child].adult_size, "子は大人より小さい");

        // エネルギーを十分に与えても、成熟するまでは繁殖しない
        let id = w.id[child];
        for _ in 0..50 {
            let c = w.index_of(id).unwrap();
            w.energy[c] = w.energy_cap(c);
            w.step();
            let c = w.index_of(id).unwrap();
            assert_eq!(w.children[c], 0, "幼体が繁殖した");
        }
    }

    #[test]
    fn 遺伝的に遠い相手とは繁殖できない() {
        let mut w = empty_world();
        let a = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let mut b = a.clone();
        // 脳の重みを大きくずらして、遺伝的に遠い相手にする
        for v in &mut b[genome::N_TRAITS..] {
            *v += 1.0;
        }
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &a);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &b);
        assert!(w.mating_chance(0, 1) < 1e-6, "相性はほぼ 0 のはず");
        w.step();
        assert_eq!(w.count(), 2, "遠い相手と繁殖した");
        assert_eq!(w.failed_matings, 1);
        assert!(w.energy[0] < 15.0, "失敗しても繁殖のコストは払う");
    }

    #[test]
    fn 近くの個体を選べる() {
        let w = World::new(2, small_params());
        let id = w.id_near(w.xs[10], w.ys[10], 0.5);
        assert_eq!(id, w.id[10]);
        assert_eq!(w.id_near(-1000.0, -1000.0, 0.5), 0);
        // 種類を指定すると、より近くに別の種類がいても指定した種類を選ぶ
        let i = (0..w.count()).find(|&i| w.kind[i] == CARNIVORE).unwrap();
        let id = w.id_near_kind(w.xs[i], w.ys[i], 1000.0, Some(HERBIVORE));
        assert_ne!(id, 0);
        assert_eq!(w.kind[w.index_of(id).unwrap()], HERBIVORE);
    }
}

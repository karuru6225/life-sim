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
}

/// 死因（観察用）
pub mod death {
    pub const STARVED: u8 = 1;
    pub const OLD_AGE: u8 = 2;
    pub const EATEN: u8 = 3;
}

/// 近傍探索のグリッドのセルの大きさ。細かいほど、視野の小さい個体が調べる範囲を絞れる
const GRID_CELL: f32 = 16.0;

/// 描画用バッファの 1 個体あたりの要素数: [x, y, 種類, 大きさ, 標識色]
pub const RENDER_STRIDE: usize = 5;

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
    /// 死んだときの (死因, tick)
    pub death: Option<(u8, u64)>,
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
    /// 遺伝子から読み出した体の形質（生まれたときに 1 度だけ計算する）
    pub traits: Vec<Traits>,
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
            params,
        };
        let p = world.params.clone();
        for (kind, count, size, speed, vision, energy) in [
            (HERBIVORE, p.herb_initial, p.herb_initial_size, p.herb_initial_speed, p.herb_initial_vision, p.herb_initial_energy),
            (CARNIVORE, p.carn_initial, p.carn_initial_size, p.carn_initial_speed, p.carn_initial_vision, p.carn_initial_energy),
        ] {
            for _ in 0..count as usize {
                let (x, y) = world.random_position();
                // 初期個体は 1 体ずつ違う標識色にする。系統の広がりが色で見える
                let hue = world.rng.f32();
                let mut g = genome::initial(kind == HERBIVORE, size, speed, vision, hue);
                genome::add_kin_wiring(&mut g, &mut world.rng);
                world.spawn(x, y, kind, energy, 0, &g);
            }
        }
        world.ledger.initial = world.total_energy();
        world
    }

    fn random_position(&mut self) -> (f32, f32) {
        (self.rng.range(0.0, self.width), self.rng.range(0.0, self.height))
    }

    fn spawn(&mut self, x: f32, y: f32, kind: u8, energy: f32, generation: u32, genome: &[f32]) {
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
        self.traits.push(genome::traits(genome, self.params.max_turn));
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
        let mut best = radius * radius;
        let mut found = 0;
        for i in 0..self.count() {
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

    pub fn repro_threshold(&self, i: usize) -> f32 {
        let base = if self.kind[i] == HERBIVORE { self.params.herb_repro_threshold } else { self.params.carn_repro_threshold };
        base * self.traits[i].size
    }

    pub fn energy_cap(&self, i: usize) -> f32 {
        self.repro_threshold(i) * self.params.energy_cap_ratio
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
            let (speed, wants_child) = self.act(i);
            self.move_agent(i, speed);
            self.live(i, speed, wants_child);
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
                }
            }
        });
        Senses { predator: best[0], prey: best[1], kin: best[2], kin_count, kin_hx, kin_hy, kin_alert }
    }

    /// 知覚 → 脳 → 行動。戻り値は (この tick の移動速度, 繁殖したいか)
    fn act(&mut self, i: usize) -> (f32, bool) {
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
        let speed = t.max_speed * throttle;
        if is_watched {
            self.watch.turn = lr.atan2(lf);
            self.watch.throttle = throttle;
        }
        let wants_eat = out[genome::OUT_EAT] > 0.0;
        let wants_child = out[genome::OUT_REPRODUCE] > 0.0;

        if self.kind[i] == HERBIVORE {
            let ate = wants_eat && self.graze(i);
            self.state[i] = if senses.predator.is_some() {
                state::FLEE
            } else if ate {
                state::GRAZE
            } else {
                state::SEEK
            };
            (speed, wants_child)
        } else {
            if wants_eat && let Some((j, _, _, d2)) = senses.prey {
                if self.try_hunt(i, j, d2) {
                    self.state[i] = state::EAT;
                    return (0.0, wants_child); // 食べた tick は動かない
                }
            }
            self.state[i] = if senses.prey.is_some() { state::CHASE } else { state::WANDER };
            (speed, wants_child)
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
    fn try_hunt(&mut self, i: usize, j: usize, d2: f32) -> bool {
        let size = self.traits[i].size;
        let range = self.params.carn_attack_range * size;
        if d2 >= range * range || self.traits[j].size > size * self.params.prey_size_ratio {
            return false;
        }
        let prey = self.energy[j];
        let room = (self.energy_cap(i) - self.energy[i]).max(0.0);
        // 食べきれない分は捨てる（熱として失われる）
        let gain = (prey * self.params.carn_efficiency).min(room);
        self.energy[i] += gain;
        self.ledger.lost_digestion += (prey - gain) as f64;
        self.energy[j] = 0.0;
        self.die(j, death::EATEN);
        true
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
    fn live(&mut self, i: usize, speed: f32, wants_child: bool) {
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
        if self.id[i] == self.watch.id {
            self.watch.speed = speed;
            self.watch.cost = cost;
        }

        if self.energy[i] <= 0.0 {
            self.energy[i] = 0.0;
            self.die(i, death::STARVED);
            return;
        }
        if self.age[i] as f32 > max_age * t.size_sqrt {
            self.ledger.lost_death += self.energy[i] as f64;
            self.energy[i] = 0.0;
            self.die(i, death::OLD_AGE);
            return;
        }
        if wants_child && self.energy[i] >= self.repro_threshold(i) {
            self.reproduce(i, birth_cost * t.size);
        }
    }

    /// 繁殖コストを引いた残りのうち、子への投資の割合を子に渡す
    fn reproduce(&mut self, i: usize, birth_cost: f32) {
        let before = self.energy[i];
        let available = before - birth_cost;
        let child = available * self.traits[i].invest;
        let parent = available - child;
        self.energy[i] = parent;
        self.ledger.lost_birth += (before - parent - child) as f64;
        self.children[i] += 1;

        let genome_at = self.births_genomes.len();
        self.births_genomes.extend_from_slice(&self.genomes[i * GENOME_LEN..(i + 1) * GENOME_LEN]);
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
            self.spawn(wrap(b.x, self.width), wrap(b.y, self.height), b.kind, b.energy, b.generation, g);
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
            sum[1] += t.size as f64;
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

    /// 群れの度合い。[集まり具合（Clark-Evans 比）, 向きのそろい具合]
    ///
    /// - 集まり具合: 最寄りの仲間までの平均距離 ÷ 同じ密度でランダムに散らばった場合の期待値（0.5 / √密度）。
    ///   1 ならランダム、1 未満なら集まっている、1 より大きければ互いに避けている
    /// - 向きのそろい具合: 自分と、半径 HERD_RADIUS 内の仲間の進行方向の cos の平均。0 ならばらばら、1 なら全員同じ向き
    ///
    /// 集まり具合だけだと「草の多い場所に別々に集まっている」のと区別できないので、向きも見る。
    /// 計算を軽くするため、HERD_SAMPLE_STEP 体に 1 体だけ調べる。
    /// グリッドを今の位置で作り直す（step の最初にも作り直すので、シミュレーションには影響しない）
    pub fn herd_stats(&mut self, kind: u8) -> [f64; 2] {
        const HERD_RADIUS: f32 = 32.0;
        const HERD_SAMPLE_STEP: usize = 4;
        /// 最寄りの仲間を探す範囲。これより遠ければこの距離として数える（疎な種類ではやや過小になる）
        const NN_LIMIT: f32 = 128.0;

        self.grid.build(&self.xs, &self.ys, &self.kind, &self.cover);
        let n_kind = self.count_kind(kind);
        if n_kind < 2 {
            return [f64::NAN, f64::NAN];
        }
        let (w, h) = (self.width, self.height);
        let (mut nn_sum, mut nn_n) = (0.0f64, 0usize);
        let (mut align_sum, mut align_n) = (0.0f64, 0usize);
        for i in (0..self.count()).step_by(HERD_SAMPLE_STEP) {
            if self.kind[i] != kind {
                continue;
            }
            let (x, y) = (self.xs[i], self.ys[i]);
            let (hx, hy) = (self.hx[i], self.hy[i]);
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
                    align_sum += (hx * self.hx[j] + hy * self.hy[j]) as f64;
                    align_n += 1;
                }
            });
            nn_sum += (best as f64).sqrt();
            nn_n += 1;
        }
        let density = n_kind as f64 / (w as f64 * h as f64);
        let expected = 0.5 / density.sqrt();
        let ratio = if nn_n > 0 { nn_sum / nn_n as f64 / expected } else { f64::NAN };
        let align = if align_n > 0 { align_sum / align_n as f64 } else { 0.0 };
        [ratio, align]
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
                assert_eq!(t.size, w.traits[i].size, "個体 {i} の遺伝子と形質がずれている");
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
    fn 近くの個体を選べる() {
        let w = World::new(2, small_params());
        let id = w.id_near(w.xs[10], w.ys[10], 0.5);
        assert_eq!(id, w.id[10]);
        assert_eq!(w.id_near(-1000.0, -1000.0, 0.5), 0);
    }
}

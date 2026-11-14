//! 世界と個体データ。
//!
//! 個体は「1 個体 = 1 構造体」ではなく、属性ごとの配列（SoA）で持つ。
//! i 番目の個体の位置は (xs[i], ys[i])、種類は kind[i]。
//! 同じ属性がメモリ上に連続して並ぶのでループが速く、JS からも配列としてそのまま読める。
//! 配列上の位置 i は死亡のたびに詰め直されて変わるので、個体を追いかけるときは id を使う。
//!
//! エネルギーは「日射 → 植物 → 草食 → 肉食」と一方通行で流れ、どこからも湧かない。
//! 出入りはすべて Ledger（帳簿）に記録し、テストで帳尻が合うことを確かめる。

use crate::grid::Grid;
use crate::params::Params;
use crate::plants::PlantField;
use crate::rng::Rng;

pub const HERBIVORE: u8 = 0;
pub const CARNIVORE: u8 = 1;

/// いまの行動（観察用）
pub mod state {
    /// 目的なく歩き回っている（肉食が獲物を探している）
    pub const WANDER: u8 = 0;
    /// その場で草を食べている
    pub const GRAZE: u8 = 1;
    /// 草の多い方へ移動している
    pub const SEEK: u8 = 2;
    /// 天敵から逃げている
    pub const FLEE: u8 = 3;
    /// 獲物を追っている
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

/// 描画用バッファの 1 個体あたりの要素数: [x, y, 種類]
pub const RENDER_STRIDE: usize = 3;

/// 目的なく歩き回るときの向きの揺れ
const WANDER_JITTER: f32 = 0.3;
/// 目標に向かうときの向きの揺れ
const STEER_JITTER: f32 = 0.15;

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
    /// 死んだときの (死因, tick)
    pub death: Option<(u8, u64)>,
}

struct Birth {
    x: f32,
    y: f32,
    kind: u8,
    energy: f32,
    generation: u32,
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
    /// 進行方向（ラジアン）
    pub headings: Vec<f32>,
    pub energy: Vec<f32>,
    pub age: Vec<u32>,
    pub kind: Vec<u8>,
    /// いまの行動（state::*）
    pub state: Vec<u8>,
    /// 初期個体が 0、その子が 1、…
    pub generation: Vec<u32>,
    pub children: Vec<u32>,
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
        // 近傍探索は周囲 1 セルまでしか見ないので、セルは最大の視野以上にする
        let grid = Grid::new(width, height, params.herb_vision.max(params.carn_vision));
        let mut world = World {
            width,
            height,
            tick: 0,
            id: Vec::new(),
            xs: Vec::new(),
            ys: Vec::new(),
            headings: Vec::new(),
            energy: Vec::new(),
            age: Vec::new(),
            kind: Vec::new(),
            state: Vec::new(),
            generation: Vec::new(),
            children: Vec::new(),
            alive: Vec::new(),
            plants,
            ledger: Ledger::default(),
            watch: Watch::default(),
            render: Vec::new(),
            grid,
            rng: Rng::new(seed),
            births: Vec::new(),
            next_id: 1,
            params,
        };
        for _ in 0..world.params.herb_initial as usize {
            let (x, y) = world.random_position();
            world.spawn(x, y, HERBIVORE, world.params.herb_initial_energy, 0);
        }
        for _ in 0..world.params.carn_initial as usize {
            let (x, y) = world.random_position();
            world.spawn(x, y, CARNIVORE, world.params.carn_initial_energy, 0);
        }
        world.ledger.initial = world.total_energy();
        world
    }

    fn random_position(&mut self) -> (f32, f32) {
        (self.rng.range(0.0, self.width), self.rng.range(0.0, self.height))
    }

    fn spawn(&mut self, x: f32, y: f32, kind: u8, energy: f32, generation: u32) {
        self.id.push(self.next_id);
        self.next_id += 1;
        self.xs.push(x);
        self.ys.push(y);
        self.headings.push(self.rng.range(0.0, std::f32::consts::TAU));
        self.energy.push(energy);
        self.age.push(0);
        self.kind.push(kind);
        self.state.push(state::WANDER);
        self.generation.push(generation);
        self.children.push(0);
        self.alive.push(true);
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    pub fn count_kind(&self, kind: u8) -> usize {
        self.kind.iter().filter(|&&k| k == kind).count()
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

    /// 1 tick 進める
    pub fn step(&mut self) {
        self.ledger.sunlight_in += self
            .plants
            .grow(self.params.sunlight, self.params.plant_capacity);

        self.grid.build(&self.xs, &self.ys);
        // セル順に回す（近くの個体を続けて処理するとキャッシュに乗って速い）
        for k in 0..self.count() {
            let i = self.grid.entry(k).0;
            if !self.alive[i] {
                continue; // この tick ですでに食べられた
            }
            let speed = match self.kind[i] {
                HERBIVORE => self.act_herbivore(i),
                _ => self.act_carnivore(i),
            };
            self.move_agent(i, speed);
            self.live(i, speed);
        }

        self.apply_births();
        self.remove_dead();
        self.tick += 1;
    }

    /// 草食の行動。天敵がいれば逃げ、いなければ草の多い方へ向かう。いまいるマスの草を食べる。
    /// 戻り値はこの tick の移動速度
    fn act_herbivore(&mut self, i: usize) -> f32 {
        let (x, y) = (self.xs[i], self.ys[i]);
        let threat = self.nearest(x, y, CARNIVORE, self.params.herb_vision);

        let speed = if let Some((_, dx, dy, _)) = threat {
            self.headings[i] = (-dy).atan2(-dx) + self.rng.range(-STEER_JITTER, STEER_JITTER);
            self.state[i] = state::FLEE;
            self.params.herb_flee_speed
        } else if let Some((dx, dy)) = self.plants.richer_direction(x, y) {
            self.headings[i] = dy.atan2(dx) + self.rng.range(-STEER_JITTER, STEER_JITTER);
            self.state[i] = state::SEEK;
            self.params.herb_speed
        } else {
            self.headings[i] += self.rng.range(-WANDER_JITTER, WANDER_JITTER);
            self.state[i] = state::GRAZE;
            self.params.herb_graze_speed
        };

        let c = self.plants.cell_index(x, y);
        let take = self.params.herb_bite.min(self.plants.biomass[c]);
        self.plants.biomass[c] -= take;
        let gain = take * self.params.herb_efficiency;
        self.energy[i] += gain;
        self.ledger.lost_digestion += (take - gain) as f64;
        speed
    }

    /// 肉食の行動。見える範囲の草食を追い、届けば食べる。
    /// 戻り値はこの tick の移動速度
    fn act_carnivore(&mut self, i: usize) -> f32 {
        let (x, y) = (self.xs[i], self.ys[i]);
        let Some((j, dx, dy, d2)) = self.nearest(x, y, HERBIVORE, self.params.carn_vision) else {
            self.headings[i] += self.rng.range(-WANDER_JITTER, WANDER_JITTER);
            self.state[i] = state::WANDER;
            return self.params.carn_wander_speed;
        };

        let range = self.params.carn_attack_range;
        if d2 < range * range {
            let prey = self.energy[j];
            let gain = prey * self.params.carn_efficiency;
            self.energy[i] += gain;
            self.ledger.lost_digestion += (prey - gain) as f64;
            self.energy[j] = 0.0;
            self.die(j, death::EATEN);
            self.state[i] = state::EAT;
            return 0.0; // 食べた tick は動かない
        }
        self.headings[i] = dy.atan2(dx) + self.rng.range(-STEER_JITTER, STEER_JITTER);
        self.state[i] = state::CHASE;
        self.params.carn_chase_speed
    }

    /// 半径 radius 内で最も近い、生きている kind の個体。(番号, dx, dy, 距離の 2 乗)
    fn nearest(&self, x: f32, y: f32, kind: u8, radius: f32) -> Option<(usize, f32, f32, f32)> {
        let (w, h) = (self.width, self.height);
        let mut best_d2 = radius * radius;
        let mut found = None;
        self.grid.for_each_nearby(x, y, |j, xj, yj| {
            if self.kind[j] == kind && self.alive[j] {
                let dx = torus_delta(xj - x, w);
                let dy = torus_delta(yj - y, h);
                let d2 = dx * dx + dy * dy;
                if d2 < best_d2 {
                    best_d2 = d2;
                    found = Some((j, dx, dy, d2));
                }
            }
        });
        found
    }

    fn move_agent(&mut self, i: usize, speed: f32) {
        if speed == 0.0 {
            return;
        }
        let h = self.headings[i];
        self.xs[i] = wrap(self.xs[i] + h.cos() * speed, self.width);
        self.ys[i] = wrap(self.ys[i] + h.sin() * speed, self.height);
    }

    /// 個体 i を死亡扱いにする。エネルギーは呼び出し側で 0 にしておくこと
    fn die(&mut self, i: usize, cause: u8) {
        self.alive[i] = false;
        if self.id[i] == self.watch.id {
            self.watch.death = Some((cause, self.tick));
        }
    }

    /// エネルギー消費・加齢・死亡・繁殖
    fn live(&mut self, i: usize, speed: f32) {
        let p = &self.params;
        let (metabolism, max_age, threshold, birth_cost) = match self.kind[i] {
            HERBIVORE => (p.herb_metabolism, p.herb_max_age, p.herb_repro_threshold, p.herb_birth_cost),
            _ => (p.carn_metabolism, p.carn_max_age, p.carn_repro_threshold, p.carn_birth_cost),
        };

        // 持っている以上は失えない（エネルギーが負になって帳尻が狂うのを防ぐ）
        let cost = (metabolism + p.move_cost * speed * speed).min(self.energy[i]);
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
        if self.age[i] as f32 > max_age {
            self.ledger.lost_death += self.energy[i] as f64;
            self.energy[i] = 0.0;
            self.die(i, death::OLD_AGE);
            return;
        }
        if self.energy[i] >= threshold {
            // 繁殖コストを引いた残りを親子で半分ずつ分ける
            let before = self.energy[i];
            let child = (before - birth_cost) / 2.0;
            self.energy[i] = child;
            self.ledger.lost_birth += (before - 2.0 * child) as f64;
            self.children[i] += 1;
            let spread = self.params.birth_spread;
            let x = self.xs[i] + self.rng.range(-spread, spread);
            let y = self.ys[i] + self.rng.range(-spread, spread);
            self.births.push(Birth {
                x,
                y,
                kind: self.kind[i],
                energy: child,
                generation: self.generation[i] + 1,
            });
        }
    }

    fn apply_births(&mut self) {
        let births = std::mem::take(&mut self.births);
        for b in &births {
            self.spawn(wrap(b.x, self.width), wrap(b.y, self.height), b.kind, b.energy, b.generation);
        }
        self.births = births;
        self.births.clear();
    }

    /// 死んだ個体を取り除く。順番は保つ（決定論のため）
    fn remove_dead(&mut self) {
        let alive = &self.alive;
        compact(&mut self.id, alive);
        compact(&mut self.xs, alive);
        compact(&mut self.ys, alive);
        compact(&mut self.headings, alive);
        compact(&mut self.energy, alive);
        compact(&mut self.age, alive);
        compact(&mut self.kind, alive);
        compact(&mut self.state, alive);
        compact(&mut self.generation, alive);
        compact(&mut self.children, alive);
        let n = self.xs.len();
        self.alive.clear();
        self.alive.resize(n, true);
    }

    pub fn fill_render(&mut self) {
        self.render.resize(self.count() * RENDER_STRIDE, 0.0);
        for i in 0..self.count() {
            let o = i * RENDER_STRIDE;
            self.render[o] = self.xs[i];
            self.render[o + 1] = self.ys[i];
            self.render[o + 2] = self.kind[i] as f32;
        }
    }
}

/// alive[i] が true の要素だけを、順番を保って前に詰める
fn compact<T: Copy>(v: &mut Vec<T>, alive: &[bool]) {
    let mut w = 0;
    for r in 0..v.len() {
        if alive[r] {
            v[w] = v[r];
            w += 1;
        }
    }
    v.truncate(w);
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
        assert_eq!(a.kind, b.kind);
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
    fn idは重複せず死亡と詰め直しの後も個体についていく() {
        let mut w = World::new(11, small_params());
        // 最後の個体を選び、他の個体の死亡で配列が詰め直されても同じ個体を指し続けるか
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

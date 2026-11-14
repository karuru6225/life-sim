//! 世界と個体データ。
//!
//! 個体は「1 個体 = 1 構造体」ではなく、属性ごとの配列（SoA）で持つ。
//! i 番目の個体の位置は (xs[i], ys[i])。
//! 同じ属性がメモリ上に連続して並ぶのでループが速く、JS からも配列としてそのまま読める。

use crate::grid::Grid;
use crate::rng::Rng;

/// 近傍とみなす距離
pub const NEIGHBOR_RADIUS: f32 = 16.0;
/// 描画用バッファの 1 個体あたりの要素数: [x, y, 近傍数]
pub const RENDER_STRIDE: usize = 3;

const SPEED: f32 = 1.2;
const TURN_JITTER: f32 = 0.35;

pub struct World {
    pub width: f32,
    pub height: f32,
    pub tick: u64,

    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    /// 進行方向（ラジアン）
    pub headings: Vec<f32>,
    /// 半径 NEIGHBOR_RADIUS 内にいる他個体の数
    pub neighbors: Vec<u16>,

    /// JS に渡す描画用データ。fill_render() で更新する
    pub render: Vec<f32>,

    grid: Grid,
    rng: Rng,
}

impl World {
    pub fn new(seed: u64, count: usize, width: f32, height: f32) -> Self {
        let mut rng = Rng::new(seed);
        let mut xs = Vec::with_capacity(count);
        let mut ys = Vec::with_capacity(count);
        let mut headings = Vec::with_capacity(count);
        for _ in 0..count {
            xs.push(rng.range(0.0, width));
            ys.push(rng.range(0.0, height));
            headings.push(rng.range(0.0, std::f32::consts::TAU));
        }
        World {
            width,
            height,
            tick: 0,
            xs,
            ys,
            headings,
            neighbors: vec![0; count],
            render: vec![0.0; count * RENDER_STRIDE],
            grid: Grid::new(width, height, NEIGHBOR_RADIUS),
            rng,
        }
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    /// 1 tick 進める
    pub fn step(&mut self) {
        self.move_agents();
        self.grid.build(&self.xs, &self.ys);
        self.count_neighbors();
        self.tick += 1;
    }

    /// ランダムウォーク。向きを少しずつ揺らしながら前に進む
    fn move_agents(&mut self) {
        for i in 0..self.count() {
            let h = self.headings[i] + self.rng.range(-TURN_JITTER, TURN_JITTER);
            self.headings[i] = h;
            self.xs[i] = wrap(self.xs[i] + h.cos() * SPEED, self.width);
            self.ys[i] = wrap(self.ys[i] + h.sin() * SPEED, self.height);
        }
    }

    fn count_neighbors(&mut self) {
        let r2 = NEIGHBOR_RADIUS * NEIGHBOR_RADIUS;
        let (w, h) = (self.width, self.height);
        for i in 0..self.count() {
            let (x, y) = (self.xs[i], self.ys[i]);
            let mut n: u16 = 0;
            self.grid.for_each_nearby(x, y, |j| {
                if j != i {
                    let dx = torus_delta(self.xs[j] - x, w);
                    let dy = torus_delta(self.ys[j] - y, h);
                    if dx * dx + dy * dy < r2 {
                        n = n.saturating_add(1);
                    }
                }
            });
            self.neighbors[i] = n;
        }
    }

    pub fn fill_render(&mut self) {
        for i in 0..self.count() {
            let o = i * RENDER_STRIDE;
            self.render[o] = self.xs[i];
            self.render[o + 1] = self.ys[i];
            self.render[o + 2] = self.neighbors[i] as f32;
        }
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
fn torus_delta(d: f32, size: f32) -> f32 {
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

    #[test]
    fn 同じシードなら同じ歴史になる() {
        let mut a = World::new(7, 500, 400.0, 300.0);
        let mut b = World::new(7, 500, 400.0, 300.0);
        for _ in 0..50 {
            a.step();
            b.step();
        }
        assert_eq!(a.xs, b.xs);
        assert_eq!(a.ys, b.ys);
    }

    #[test]
    fn 近傍数が総当たりと一致する() {
        let mut w = World::new(3, 2000, 320.0, 240.0);
        for _ in 0..10 {
            w.step();
        }
        let r2 = NEIGHBOR_RADIUS * NEIGHBOR_RADIUS;
        for i in 0..w.count() {
            let mut expected = 0;
            for j in 0..w.count() {
                if i == j {
                    continue;
                }
                let dx = torus_delta(w.xs[j] - w.xs[i], w.width);
                let dy = torus_delta(w.ys[j] - w.ys[i], w.height);
                if dx * dx + dy * dy < r2 {
                    expected += 1;
                }
            }
            assert_eq!(w.neighbors[i], expected, "個体 {i}");
        }
    }

    #[test]
    fn 座標は世界の中に収まる() {
        let mut w = World::new(9, 1000, 200.0, 100.0);
        for _ in 0..200 {
            w.step();
        }
        assert!(w.xs.iter().all(|&x| (0.0..200.0).contains(&x)));
        assert!(w.ys.iter().all(|&y| (0.0..100.0).contains(&y)));
    }
}

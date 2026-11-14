//! 植物。個体ではなく、世界をマス目に切った「マスごとの植物量」として持つ。
//!
//! 毎 tick、各マスは日射を受けて植物量が増える。ただし満杯（capacity）に近いほど
//! 受け取れる量が減る（葉が重なって光を使い切れない、のような飽和）。
//! 受け取った分が生態系へのエネルギー流入になる。

pub struct PlantField {
    pub cols: usize,
    pub rows: usize,
    pub cell_size: f32,
    /// マスごとの植物量。i = row * cols + col
    pub biomass: Vec<f32>,
}

impl PlantField {
    pub fn new(width: f32, height: f32, cell_size: f32, initial: f32) -> Self {
        let cols = ((width / cell_size).ceil() as usize).max(1);
        let rows = ((height / cell_size).ceil() as usize).max(1);
        PlantField { cols, rows, cell_size, biomass: vec![initial; cols * rows] }
    }

    /// 日射を受けて成長する。受け取ったエネルギーの合計を返す
    pub fn grow(&mut self, sunlight: f32, capacity: f32) -> f64 {
        let mut total = 0.0f64;
        for b in &mut self.biomass {
            let g = sunlight * (1.0 - *b / capacity).max(0.0);
            *b += g;
            total += g as f64;
        }
        total
    }

    pub fn cell_index(&self, x: f32, y: f32) -> usize {
        let c = ((x / self.cell_size) as usize).min(self.cols - 1);
        let r = ((y / self.cell_size) as usize).min(self.rows - 1);
        r * self.cols + c
    }

    /// 周囲 8 マスのうち、いまいるマスより植物が多いマスがあれば、その方向（単位ベクトルでない）を返す
    pub fn richer_direction(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let c = ((x / self.cell_size) as usize).min(self.cols - 1) as isize;
        let r = ((y / self.cell_size) as usize).min(self.rows - 1) as isize;
        let here = self.biomass[r as usize * self.cols + c as usize];
        let mut best = here * 1.1 + 0.05;
        let mut dir = None;
        for dr in -1..=1isize {
            for dc in -1..=1isize {
                if dr == 0 && dc == 0 {
                    continue;
                }
                // トーラスなので端は反対側につながる
                let rr = (r + dr).rem_euclid(self.rows as isize) as usize;
                let cc = (c + dc).rem_euclid(self.cols as isize) as usize;
                let b = self.biomass[rr * self.cols + cc];
                if b > best {
                    best = b;
                    dir = Some((dc as f32, dr as f32));
                }
            }
        }
        dir
    }

    pub fn total(&self) -> f64 {
        self.biomass.iter().map(|&b| b as f64).sum()
    }
}

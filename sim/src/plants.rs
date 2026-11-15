//! 植物。個体ではなく、世界をマス目に切った「マスごとの植物量」として持つ。
//!
//! 毎 tick、各マスは日射を受けて植物量が増える。ただし満杯（capacity）に近いほど
//! 受け取れる量が減る（葉が重なって光を使い切れない、のような飽和）。
//! 受け取った分が生態系へのエネルギー流入になる。
//! マスごとの肥沃度（地形が決める）が、溜まる上限と育つ速さの両方に掛かる。水のマス（肥沃度 0）では育たない。

pub struct PlantField {
    pub cols: usize,
    pub rows: usize,
    pub cell_size: f32,
    /// マスごとの植物量。i = row * cols + col
    pub biomass: Vec<f32>,
    /// マスごとの肥沃度（0〜1）。季節とともに変わる（World が地形から書き込む）
    pub fertility: Vec<f32>,
    /// マスごとの植生の種類（土の肥えかた、0〜1）。季節で変わらない。地形の soil と同じ並び
    pub vegetation: Vec<f32>,
}

impl PlantField {
    /// fertility は地形のマスごとの肥沃度、vegetation は植生の種類（どちらも cols × rows）。初期の植物量は initial × 肥沃度
    pub fn new(width: f32, height: f32, cell_size: f32, initial: f32, fertility: &[f32], vegetation: &[f32]) -> Self {
        let cols = ((width / cell_size).ceil() as usize).max(1);
        let rows = ((height / cell_size).ceil() as usize).max(1);
        assert_eq!(fertility.len(), cols * rows, "地形と植物のマス目がずれている");
        let biomass = fertility.iter().map(|&f| initial * f).collect();
        PlantField { cols, rows, cell_size, biomass, fertility: fertility.to_vec(), vegetation: vegetation.to_vec() }
    }

    /// 日射を受けて成長する。受け取ったエネルギーの合計を返す。
    /// 肥沃度 f のマスは、上限が capacity × f、受け取る日射も sunlight × f になる
    pub fn grow(&mut self, sunlight: f32, capacity: f32) -> f64 {
        let mut total = 0.0f64;
        for (b, &f) in self.biomass.iter_mut().zip(&self.fertility) {
            if f <= 0.0 {
                continue; // 水
            }
            let g = sunlight * f * (1.0 - *b / (capacity * f)).max(0.0);
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

    /// (x, y) のマスの植物量を上限に対する割合（0〜1）で返す
    pub fn ratio_at(&self, x: f32, y: f32, capacity: f32) -> f32 {
        (self.biomass[self.cell_index(x, y)] / capacity).clamp(0.0, 1.0)
    }

    /// 周囲 8 マスのうち、いまいるマスより植物が多いマスがあれば、その方向（単位ベクトルでない）を返す
    pub fn richer_direction(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        self.richer_direction_for(x, y, |_| 1.0)
    }

    /// richer_direction と同じだが、マスの植物量に weight(マスの植生の種類) を掛けて比べる。
    /// 食べられる植生が決まっている草食が、自分に合う草を探すときに使う
    pub fn richer_direction_for(&self, x: f32, y: f32, weight: impl Fn(f32) -> f32) -> Option<(f32, f32)> {
        let c = ((x / self.cell_size) as usize).min(self.cols - 1);
        let r = ((y / self.cell_size) as usize).min(self.rows - 1);
        let value = |k: usize| self.biomass[k] * weight(self.vegetation[k]);
        let here = value(r * self.cols + c);
        let mut best = here * 1.1 + 0.05;
        let mut dir = None;
        // トーラスなので端は反対側につながる。剰余（%）は遅いので分岐で折り返す
        let rows = [if r == 0 { self.rows - 1 } else { r - 1 }, r, if r + 1 == self.rows { 0 } else { r + 1 }];
        let cols = [if c == 0 { self.cols - 1 } else { c - 1 }, c, if c + 1 == self.cols { 0 } else { c + 1 }];
        for (dr, &rr) in rows.iter().enumerate() {
            for (dc, &cc) in cols.iter().enumerate() {
                if dr == 1 && dc == 1 {
                    continue;
                }
                let b = value(rr * self.cols + cc);
                if b > best {
                    best = b;
                    dir = Some((dc as f32 - 1.0, dr as f32 - 1.0));
                }
            }
        }
        dir
    }

    pub fn total(&self) -> f64 {
        self.biomass.iter().map(|&b| b as f64).sum()
    }
}

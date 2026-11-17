//! 植物。個体ではなく、世界をマス目に切った「マスごとの植物量」として持つ。
//!
//! 毎 tick、各マスは日射を受けて植物量が増える。ただし満杯（capacity）に近いほど
//! 受け取れる量が減る（葉が重なって光を使い切れない、のような飽和）。
//! 受け取った分が生態系へのエネルギー流入になる。
//! マスごとの肥沃度（地形が決める）が、溜まる上限と育つ速さの両方に掛かる。水のマス（肥沃度 0）では育たない。
//!
//! 各マスの植物は高さの層（地面の草・低い茂み・高い枝）に分かれ、日射と上限を層の割合で分けて、それぞれ育つ。
//! 高い層は大きい体だけが届く（届く割合は World が決める）。

/// 高さの層の数（0: 地面の草、1: 低い茂み、2: 高い枝）
pub const LAYERS: usize = 3;

pub struct PlantField {
    pub cols: usize,
    pub rows: usize,
    pub cell_size: f32,
    /// マスごとの植物量（層の合計）。i = row * cols + col。層を変えたら必ず合わせる
    pub biomass: Vec<f32>,
    /// 層ごとのマスごとの植物量 [層][マス]
    pub layer: [Vec<f32>; LAYERS],
    /// 層ごとの、日射と上限の割合（合計 1）
    pub share: [f32; LAYERS],
    /// マスごとの肥沃度（0〜1）。季節とともに変わる（World が地形から書き込む）
    pub fertility: Vec<f32>,
    /// マスごとの植生の種類（土の肥えかた、0〜1）。季節で変わらない。地形の soil と同じ並び
    pub vegetation: Vec<f32>,
}

impl PlantField {
    /// fertility は地形のマスごとの肥沃度、vegetation は植生の種類（どちらも cols × rows）。初期の植物量は initial × 肥沃度
    /// share は層ごとの日射と上限の割合（合計 1 になるように使う）
    pub fn new(
        width: f32,
        height: f32,
        cell_size: f32,
        initial: f32,
        fertility: &[f32],
        vegetation: &[f32],
        share: [f32; LAYERS],
    ) -> Self {
        let cols = ((width / cell_size).ceil() as usize).max(1);
        let rows = ((height / cell_size).ceil() as usize).max(1);
        assert_eq!(fertility.len(), cols * rows, "地形と植物のマス目がずれている");
        let mut field = PlantField {
            cols,
            rows,
            cell_size,
            biomass: vec![0.0; cols * rows],
            layer: std::array::from_fn(|_| vec![0.0; cols * rows]),
            share,
            fertility: fertility.to_vec(),
            vegetation: vegetation.to_vec(),
        };
        for c in 0..cols * rows {
            field.set_cell(c, initial * fertility[c]);
        }
        field
    }

    /// マス c の植物量を、層の割合で分けて amount にする（初期化と試験用）
    pub fn set_cell(&mut self, c: usize, amount: f32) {
        for k in 0..LAYERS {
            self.layer[k][c] = amount * self.share[k];
        }
        self.biomass[c] = amount;
    }

    /// すべてのマスの植物量を amount にする（層の割合で分ける。試験用）
    pub fn fill(&mut self, amount: f32) {
        for c in 0..self.biomass.len() {
            self.set_cell(c, amount);
        }
    }

    /// マス c の層 k から amount を取り除く（呼ぶ側で、層の量を超えないようにする）
    pub fn take(&mut self, c: usize, k: usize, amount: f32) {
        self.layer[k][c] -= amount;
        self.biomass[c] -= amount;
    }

    /// 層の和から biomass を作り直す（層を直接書き換えた試験用）
    pub fn sum_layers(&mut self) {
        for c in 0..self.biomass.len() {
            self.biomass[c] = (0..LAYERS).map(|k| self.layer[k][c]).sum();
        }
    }

    /// 日射を受けて成長する。受け取ったエネルギーの合計を返す。
    /// 肥沃度 f のマスは、上限が capacity × f、受け取る日射も sunlight × f になる
    pub fn grow(&mut self, sunlight: f32, capacity: f32) -> f64 {
        // 層ごとに、日射と上限を割合で分けて育てる。割合が全部同じ層なら、層に分けないのと同じ量になる
        let mut total = 0.0f64;
        for (c, &f) in self.fertility.iter().enumerate() {
            if f <= 0.0 {
                continue; // 水
            }
            let mut grown = 0.0;
            for k in 0..LAYERS {
                let s = self.share[k];
                if s <= 0.0 {
                    continue;
                }
                let b = &mut self.layer[k][c];
                let g = sunlight * s * f * (1.0 - *b / (capacity * s * f)).max(0.0);
                *b += g;
                grown += g;
            }
            self.biomass[c] += grown;
            total += grown as f64;
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
        self.richer_direction_by(x, y, |k| self.biomass[k])
    }

    /// richer_direction と同じだが、マスの価値を value(マスの番号) で比べる。
    /// 届く層や食べられる植生が決まっている草食が、自分に合う草を探すときに使う。
    /// value は植物量と同じ尺度（上限の大きさ）で返す
    pub fn richer_direction_by(&self, x: f32, y: f32, value: impl Fn(usize) -> f32) -> Option<(f32, f32)> {
        let c = ((x / self.cell_size) as usize).min(self.cols - 1);
        let r = ((y / self.cell_size) as usize).min(self.rows - 1);
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

//! 植物。個体ではなく、世界をマス目に切った「マスごとの植物量」として持つ。
//!
//! 毎 tick、各マスは日射を受けて植物量が増える。ただし満杯（capacity）に近いほど
//! 受け取れる量が減る（葉が重なって光を使い切れない、のような飽和）。
//! 受け取った分が生態系へのエネルギー流入になる。
//! マスごとの肥沃度（地形が決める）が、溜まる上限と育つ速さの両方に掛かる。水のマス（肥沃度 0）では育たない。
//!
//! 各マスの植物は高さの層（地面の草・低い茂み・高い枝）に分かれ、日射と上限を層の割合で分けて、それぞれ育つ。
//! 高い層は大きい体だけが届く（届く割合は World が決める）。
//! 層の割合は場所ごとに違う。土の肥えた場所ほど茂みと枝が多い林に、やせた場所ほど地面の草が多い草原になる
//! （植生の種類で決まり、季節で変わらない）。

/// 高さの層の数（0: 地面の草、1: 低い茂み、2: 高い枝）
pub const LAYERS: usize = 3;
/// 遠くの景色（粗い地図）の 1 区画の、1 辺のマスの数
pub const BLOCK: usize = 4;

/// 遠くの景色: マスを BLOCK × BLOCK ずつまとめた粗い地図。遠くを見るときは、マスではなくこの区画の平均を読む
#[derive(Default)]
pub struct Landscape {
    pub cols: usize,
    pub rows: usize,
    /// 区画ごとの層ごとの植物量の平均（水のマスも 0 として平均に入る）[層][区画]
    pub layer: [Vec<f32>; LAYERS],
    /// 区画ごとの、地面の層の上限の割合（肥沃度 × 地面の層の割合）の平均（水のマスも 0 として入る）。植物の上限を掛けると地面の層の上限
    pub ground_cap: Vec<f32>,
    /// 区画ごとの植生の種類の平均（陸のマスだけで平均する。陸がなければ −1）
    pub vegetation: Vec<f32>,
}

pub struct PlantField {
    pub cols: usize,
    pub rows: usize,
    pub cell_size: f32,
    /// マスごとの植物量（層の合計）。i = row * cols + col。層を変えたら必ず合わせる
    pub biomass: Vec<f32>,
    /// 層ごとのマスごとの植物量 [層][マス]
    pub layer: [Vec<f32>; LAYERS],
    /// 層ごとの、日射と上限の割合の全体の平均（合計 1）。マスごとの割合は share_at
    pub share: [f32; LAYERS],
    /// 林と草原の差の大きさ（0 ならどこも share、1 なら最もやせた場所で茂みと枝がなくなり、最も肥えた場所で 2 倍）
    pub woodland: f32,
    /// マスごとの林の度合い（0: 最も草原、1: 最も林）。陸のマスの植生の種類の順位で決める（植生の分布によらず一様に散らばる）
    pub wood: Vec<f32>,
    /// マスごとの肥沃度（0〜1）。季節とともに変わる（World が地形から書き込む）
    pub fertility: Vec<f32>,
    /// マスごとの植生の種類（土の肥えかた、0〜1）。季節で変わらない。地形の soil と同じ並び
    pub vegetation: Vec<f32>,
    /// 遠くの景色（update_landscape() で作り直す）
    pub landscape: Landscape,
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
        woodland: f32,
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
            woodland,
            wood: wood_ranks(fertility, vegetation),
            fertility: fertility.to_vec(),
            vegetation: vegetation.to_vec(),
            landscape: Landscape::default(),
        };
        for c in 0..cols * rows {
            field.set_cell(c, initial * fertility[c]);
        }
        field
    }

    /// 遠くの景色（区画ごとの平均）を、今の植物量と肥沃度から作り直す
    pub fn update_landscape(&mut self) {
        let (bc, br) = (self.cols.div_ceil(BLOCK), self.rows.div_ceil(BLOCK));
        let ground: Vec<f32> = (0..self.cols * self.rows).map(|i| self.share_at(i)[0] * self.fertility[i]).collect();
        let ls = &mut self.landscape;
        if ls.cols != bc || ls.rows != br {
            *ls = Landscape {
                cols: bc,
                rows: br,
                layer: std::array::from_fn(|_| vec![0.0; bc * br]),
                ground_cap: vec![0.0; bc * br],
                vegetation: vec![0.0; bc * br],
            };
        }
        for b in 0..bc * br {
            let (c0, r0) = ((b % bc) * BLOCK, (b / bc) * BLOCK);
            let mut sum = [0.0f32; LAYERS];
            let (mut fert, mut veg, mut cells, mut land) = (0.0f32, 0.0f32, 0u32, 0u32);
            for r in r0..(r0 + BLOCK).min(self.rows) {
                for c in c0..(c0 + BLOCK).min(self.cols) {
                    let i = r * self.cols + c;
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += self.layer[k][i];
                    }
                    fert += ground[i];
                    cells += 1;
                    // 植生の種類は陸だけで平均する（水が混ざると別の植生に見えてしまう）
                    if self.fertility[i] > 0.0 || self.biomass[i] > 0.0 {
                        veg += self.vegetation[i];
                        land += 1;
                    }
                }
            }
            let n = cells.max(1) as f32;
            for k in 0..LAYERS {
                ls.layer[k][b] = sum[k] / n;
            }
            ls.ground_cap[b] = fert / n;
            ls.vegetation[b] = if land > 0 { veg / land as f32 } else { -1.0 };
        }
    }

    /// 世界の座標 (x, y) を含む遠くの景色の区画（トーラスなので世界の外は反対側に折り返す）
    pub fn landscape_block(&self, x: f32, y: f32) -> usize {
        let (w, h) = (self.cols as f32 * self.cell_size, self.rows as f32 * self.cell_size);
        let c = ((x.rem_euclid(w) / self.cell_size) as usize).min(self.cols - 1) / BLOCK;
        let r = ((y.rem_euclid(h) / self.cell_size) as usize).min(self.rows - 1) / BLOCK;
        r * self.landscape.cols + c
    }

    /// マス c の層ごとの、日射と上限の割合（合計 1）。林の度合いが高いほど茂みと枝が多い
    pub fn share_at(&self, c: usize) -> [f32; LAYERS] {
        let m = 1.0 + self.woodland.clamp(0.0, 1.0) * (2.0 * self.wood[c] - 1.0);
        let (mut shrub, mut tree) = (self.share[1] * m, self.share[2] * m);
        // 地面の層は少なくとも 5% 残す
        let upper = shrub + tree;
        if upper > 0.95 {
            shrub *= 0.95 / upper;
            tree *= 0.95 / upper;
        }
        [1.0 - shrub - tree, shrub, tree]
    }

    /// マス c の植物量を、層の割合で分けて amount にする（初期化と試験用）
    pub fn set_cell(&mut self, c: usize, amount: f32) {
        let share = self.share_at(c);
        for k in 0..LAYERS {
            self.layer[k][c] = amount * share[k];
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
    /// shape は育ち方の形（params の plant_regrowth_shape）。0 は空に近いほど速く、1 はロジスティック成長
    pub fn grow(&mut self, sunlight: f32, capacity: f32, shape: f32) -> f64 {
        let shape = shape.clamp(0.0, 1.0);
        // 層ごとに、日射と上限を割合で分けて育てる。割合が全部同じ層なら、層に分けないのと同じ量になる
        let mut total = 0.0f64;
        for (c, &f) in self.fertility.iter().enumerate() {
            if f <= 0.0 {
                continue; // 水
            }
            let mut grown = 0.0;
            let share = self.share_at(c);
            for k in 0..LAYERS {
                let s = share[k];
                if s <= 0.0 {
                    continue;
                }
                let b = &mut self.layer[k][c];
                let x = *b / (capacity * s * f);
                let rate = if shape > 0.0 { (1.0 - shape) + shape * 4.0 * x.max(0.01) } else { 1.0 };
                let g = sunlight * s * f * (1.0 - x).max(0.0) * rate;
                *b += g;
                grown += g;
            }
            self.biomass[c] += grown;
            total += grown as f64;
        }
        total
    }

    /// マス c の地面の層の上限の、植物の上限に対する割合（肥沃度 × 地面の層の割合）
    pub fn ground_cap(&self, c: usize) -> f32 {
        self.share_at(c)[0] * self.fertility[c]
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

/// 陸のマスの林の度合い（0〜1）を、植生の種類の順位で決める。同じ値は同じ度合い。水のマスと、植生がどこも同じなら 0.5
fn wood_ranks(fertility: &[f32], vegetation: &[f32]) -> Vec<f32> {
    let mut land: Vec<usize> = (0..vegetation.len()).filter(|&i| fertility[i] > 0.0).collect();
    let mut wood = vec![0.5; vegetation.len()];
    if land.len() < 2 {
        return wood;
    }
    land.sort_by(|&a, &b| vegetation[a].total_cmp(&vegetation[b]));
    let n = land.len() as f32;
    let mut k = 0;
    while k < land.len() {
        // 同じ値の並びには、その並びの順位の平均を使う
        let mut e = k;
        while e + 1 < land.len() && vegetation[land[e + 1]] == vegetation[land[k]] {
            e += 1;
        }
        let rank = ((k + e) as f32 / 2.0 + 0.5) / n;
        for &i in &land[k..=e] {
            wood[i] = rank;
        }
        k = e + 1;
    }
    if (vegetation[land[0]] - vegetation[land[land.len() - 1]]).abs() < 1e-6 {
        wood.fill(0.5);
    }
    wood
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 土の肥えた場所ほど茂みと枝が多く全体の平均は変わらない() {
        // 4 マスの陸（土の肥えかた 0.3〜0.9）と 1 マスの水
        let fertility = [1.0, 1.0, 1.0, 1.0, 0.0];
        let vegetation = [0.9, 0.3, 0.5, 0.7, 0.0];
        let field = PlantField::new(5.0, 1.0, 1.0, 0.0, &fertility, &vegetation, [0.5, 0.3, 0.2], 0.8);
        let shares: Vec<[f32; LAYERS]> = (0..4).map(|c| field.share_at(c)).collect();
        // 最もやせたマス（1。順位 0.125）は草原、最も肥えたマス（0。順位 0.875）は林
        assert!(shares[1][0] > 0.75 && shares[1][2] < 0.1, "草原: {:?}", shares[1]);
        assert!(shares[0][0] < 0.25 && shares[0][2] > 0.3, "林: {:?}", shares[0]);
        for s in &shares {
            assert!((s.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
        // 陸の平均は全体の割合と同じ
        for k in 0..LAYERS {
            let mean = shares.iter().map(|s| s[k]).sum::<f32>() / 4.0;
            assert!((mean - field.share[k]).abs() < 1e-5, "層 {k}: {mean}");
        }
        // 差をなくせばどこも同じ
        let flat = PlantField::new(5.0, 1.0, 1.0, 0.0, &fertility, &vegetation, [0.5, 0.3, 0.2], 0.0);
        let s = flat.share_at(1);
        assert!((s[0] - 0.5).abs() < 1e-6 && s[1] == 0.3 && s[2] == 0.2, "{s:?}");
    }
}

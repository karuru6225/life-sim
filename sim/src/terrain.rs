//! 地形。植物と同じマス目で、マスごとに「水か陸か」「肥沃度」「平均気温」を持つ。
//!
//! 標高と肥沃度は、乱数の種から作るノイズ（値ノイズを何層か重ねたもの）で決める。
//! 世界は上下左右がつながっている（トーラス）ので、ノイズも端と端が自然につながるように作る。
//! 水域は「標高の低いほうから water_fraction の割合のマス」とする。割合で決めるので、
//! どの種でも水の量がそろう。
//!
//! 平均気温は、緯度（南北の位置）と標高で決まり、小さなむらが乗る。世界は上下がつながっているので、
//! 緯度による差は cos で表し、暖かい帯と寒い帯が南北に並ぶ。植物は適温から離れるほど育ちにくいので、
//! 肥沃度（＝植生）には気候の影響も含める。

use crate::rng::Rng;

pub struct Terrain {
    pub cols: usize,
    pub rows: usize,
    pub cell_size: f32,
    /// マスが水なら true。個体は入れず、植物も育たない
    pub water: Vec<bool>,
    /// 肥沃度（陸は fertility_min〜1、水は 0）。植物が溜まる上限と育つ速さに掛かる
    pub fertility: Vec<f32>,
    /// 陸のマスの数
    pub land_cells: usize,
    /// 平均気温（度）
    pub temperature: Vec<f32>,
}

/// 地形を作るときの設定
pub struct TerrainSpec {
    /// 水にするマスの割合（0 なら水域なし）
    pub water_fraction: f32,
    /// 標高の起伏の大きさ（いちばん大きな山や湖のおおよその大きさ、px）
    pub elevation_scale: f32,
    /// 肥沃度のむらの大きさ（px）
    pub fertility_scale: f32,
    /// 陸の肥沃度の下限
    pub fertility_min: f32,
    /// 平均気温の全体の平均（度）
    pub temp_mean: f32,
    /// 緯度による気温差の振れ幅（度）。南北で ± この値
    pub temp_latitude: f32,
    /// 標高による気温の低下（陸の最も高い所で何度下がるか）
    pub temp_lapse: f32,
    /// 気温のむらの大きさ（度）
    pub temp_noise: f32,
    /// 植物の適温（度）と、適温からの離れにくさの幅（度）。離れるほど育ちにくい
    pub plant_temp_opt: f32,
    pub plant_temp_width: f32,
}

impl Terrain {
    pub fn generate(width: f32, height: f32, cell_size: f32, spec: &TerrainSpec, rng: &mut Rng) -> Self {
        let cols = ((width / cell_size).ceil() as usize).max(1);
        let rows = ((height / cell_size).ceil() as usize).max(1);
        let elevation = fbm(cols, rows, cell_size, spec.elevation_scale, rng);
        let fert_noise = fbm(cols, rows, cell_size, spec.fertility_scale, rng);

        // 標高の低いほうから water_fraction の割合を水にする
        let mut sorted = elevation.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let n_water = ((cols * rows) as f32 * spec.water_fraction.clamp(0.0, 0.95)) as usize;
        let sea_level = if n_water == 0 { f32::NEG_INFINITY } else { sorted[n_water - 1] };
        let water: Vec<bool> = elevation.iter().map(|&e| e <= sea_level).collect();

        // 平均気温: 緯度（cos で上下がつながる）− 標高（海面からの高さ）＋ むら
        let temp_noise = fbm(cols, rows, cell_size, spec.elevation_scale * 0.7, rng);
        let land_top = sorted[sorted.len() - 1];
        let temperature: Vec<f32> = (0..cols * rows)
            .map(|k| {
                let r = k / cols;
                let latitude = (std::f32::consts::TAU * (r as f32 + 0.5) / rows as f32).cos();
                let height = if water[k] { 0.0 } else { (elevation[k] - sea_level) / (land_top - sea_level).max(1e-6) };
                spec.temp_mean + spec.temp_latitude * latitude - spec.temp_lapse * height
                    + spec.temp_noise * (2.0 * temp_noise[k] - 1.0)
            })
            .collect();

        // 肥沃度（植生）= 土の肥えかた × 気温による育ちやすさ
        let fertility = (0..cols * rows)
            .map(|k| {
                if water[k] {
                    return 0.0;
                }
                let soil = spec.fertility_min + (1.0 - spec.fertility_min) * fert_noise[k];
                let t = (temperature[k] - spec.plant_temp_opt) / spec.plant_temp_width;
                soil * (-t * t).exp()
            })
            .collect();
        let land_cells = water.iter().filter(|&&w| !w).count();
        Terrain { cols, rows, cell_size, water, fertility, land_cells, temperature }
    }

    /// 地形なし（全面が肥沃度 1 の陸で、気温はどこも temp_mean）
    pub fn flat(width: f32, height: f32, cell_size: f32, temp_mean: f32) -> Self {
        let cols = ((width / cell_size).ceil() as usize).max(1);
        let rows = ((height / cell_size).ceil() as usize).max(1);
        let n = cols * rows;
        Terrain {
            cols,
            rows,
            cell_size,
            water: vec![false; n],
            fertility: vec![1.0; n],
            land_cells: n,
            temperature: vec![temp_mean; n],
        }
    }

    pub fn temperature_at(&self, x: f32, y: f32) -> f32 {
        self.temperature[self.cell_index(x, y)]
    }

    pub fn cell_index(&self, x: f32, y: f32) -> usize {
        let c = ((x / self.cell_size) as usize).min(self.cols - 1);
        let r = ((y / self.cell_size) as usize).min(self.rows - 1);
        r * self.cols + c
    }

    pub fn is_water(&self, x: f32, y: f32) -> bool {
        self.water[self.cell_index(x, y)]
    }

    /// 陸の面積（px²）
    pub fn land_area(&self) -> f32 {
        self.land_cells as f32 * self.cell_size * self.cell_size
    }
}

/// 端と端がつながる値ノイズを、細かさを倍々にしながら 4 層重ねたもの（fBm）。値は 0〜1 に正規化する。
/// scale は最も粗い層の起伏の大きさ（px）
fn fbm(cols: usize, rows: usize, cell_size: f32, scale: f32, rng: &mut Rng) -> Vec<f32> {
    let mut out = vec![0.0f32; cols * rows];
    let mut amplitude = 1.0;
    for octave in 0..4 {
        // この層の格子の数。世界の幅（マス数）を割り切れなくても、格子の数で割った位置で補間するので端はつながる
        let freq = (1 << octave) as f32;
        let lx = ((cols as f32 * cell_size / scale * freq).round() as usize).max(2);
        let ly = ((rows as f32 * cell_size / scale * freq).round() as usize).max(2);
        let lattice: Vec<f32> = (0..lx * ly).map(|_| rng.f32()).collect();
        for r in 0..rows {
            for c in 0..cols {
                // マスの中心が、格子のどこにあたるか（0〜lx）
                let u = (c as f32 + 0.5) / cols as f32 * lx as f32;
                let v = (r as f32 + 0.5) / rows as f32 * ly as f32;
                out[r * cols + c] += amplitude * sample(&lattice, lx, ly, u, v);
            }
        }
        amplitude *= 0.5;
    }
    let (min, max) = out.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (max - min).max(1e-6);
    out.iter_mut().for_each(|v| *v = (*v - min) / span);
    out
}

/// 格子の値を、なめらかに補間して読む（端は反対側につながる）
fn sample(lattice: &[f32], lx: usize, ly: usize, u: f32, v: f32) -> f32 {
    let (x0, y0) = (u.floor() as usize % lx, v.floor() as usize % ly);
    let (x1, y1) = ((x0 + 1) % lx, (y0 + 1) % ly);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (smooth(u.fract()), smooth(v.fract()));
    let at = |x: usize, y: usize| lattice[y * lx + x];
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
    top + (bottom - top) * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TerrainSpec {
        TerrainSpec {
            water_fraction: 0.25,
            elevation_scale: 600.0,
            fertility_scale: 400.0,
            fertility_min: 0.3,
            temp_mean: 15.0,
            temp_latitude: 12.0,
            temp_lapse: 15.0,
            temp_noise: 3.0,
            plant_temp_opt: 18.0,
            plant_temp_width: 15.0,
        }
    }

    #[test]
    fn 気温は南北で差があり高い所ほど低い() {
        let t = Terrain::generate(4800.0, 3200.0, 32.0, &spec(), &mut Rng::new(7));
        let row_mean = |r: usize| (0..t.cols).map(|c| t.temperature[r * t.cols + c]).sum::<f32>() / t.cols as f32;
        // 上端（cos ≈ 1）は暖かく、中央（cos ≈ −1）は寒い
        assert!(row_mean(0) > row_mean(t.rows / 2) + 15.0);
    }

    #[test]
    fn 水域の割合は指定どおり() {
        let t = Terrain::generate(4800.0, 3200.0, 32.0, &spec(), &mut Rng::new(1));
        let ratio = 1.0 - t.land_cells as f32 / t.water.len() as f32;
        assert!((ratio - 0.25).abs() < 0.01, "水域の割合 {ratio}");
    }

    #[test]
    fn 肥沃度は陸で0から1の間_水で0() {
        let t = Terrain::generate(4800.0, 3200.0, 32.0, &spec(), &mut Rng::new(2));
        for (k, &f) in t.fertility.iter().enumerate() {
            if t.water[k] {
                assert_eq!(f, 0.0);
            } else {
                assert!((0.0..=1.0).contains(&f), "肥沃度 {f}");
            }
        }
    }

    #[test]
    fn 端と端がなめらかにつながる() {
        let t = Terrain::generate(4800.0, 3200.0, 32.0, &spec(), &mut Rng::new(3));
        // 左端の列と右端の列の肥沃度の差は、隣り合う列どうしの差と同じくらい小さい
        let (c, r) = (t.cols, t.rows);
        let edge: f32 = (0..r).map(|y| (t.fertility[y * c] - t.fertility[y * c + c - 1]).abs()).sum::<f32>() / r as f32;
        let inner: f32 = (0..r).map(|y| (t.fertility[y * c + c / 2] - t.fertility[y * c + c / 2 - 1]).abs()).sum::<f32>() / r as f32;
        assert!(edge < inner * 3.0 + 0.02, "端の差 {edge} / 内側の差 {inner}");
    }

    #[test]
    fn 同じ種なら同じ地形() {
        let a = Terrain::generate(960.0, 640.0, 32.0, &spec(), &mut Rng::new(5));
        let b = Terrain::generate(960.0, 640.0, 32.0, &spec(), &mut Rng::new(5));
        assert_eq!(a.water, b.water);
        assert_eq!(a.fertility, b.fertility);
    }
}

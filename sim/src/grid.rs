//! 空間ハッシュ（一様グリッド）。近傍探索を O(n²) から O(n) 近くに落とす。
//!
//! 毎 tick、全個体をセルごとに並べ直す（カウンティングソート）。
//! `items[cell_start[c] .. cell_start[c + 1]]` がセル c に入っている個体の番号になる。
//! 世界はトーラス（端がつながっている）なので、端のセルは反対側のセルと隣接する。

pub struct Grid {
    pub cell_size: f32,
    pub cols: usize,
    pub rows: usize,
    cell_start: Vec<u32>,
    items: Vec<u32>,
    /// items と同じ並び（セル順）にした座標のコピー。
    /// 近傍探索で座標をメモリ上の連続した位置から読めるので、キャッシュに乗りやすい
    sorted_x: Vec<f32>,
    sorted_y: Vec<f32>,
    cursor: Vec<u32>,
}

impl Grid {
    pub fn new(width: f32, height: f32, cell_size: f32) -> Self {
        // セルが cell_size より小さくならないよう切り捨てる。
        // 3 未満だと周囲 3x3 の探索で同じセルを二重に数えるため、最低 3 にする。
        let cols = ((width / cell_size).floor() as usize).max(3);
        let rows = ((height / cell_size).floor() as usize).max(3);
        Grid {
            cell_size: (width / cols as f32).max(height / rows as f32),
            cols,
            rows,
            cell_start: vec![0; cols * rows + 1],
            items: Vec::new(),
            sorted_x: Vec::new(),
            sorted_y: Vec::new(),
            cursor: vec![0; cols * rows],
        }
    }

    fn cell_xy(&self, x: f32, y: f32) -> (usize, usize) {
        let cx = ((x / self.cell_size) as usize).min(self.cols - 1);
        let cy = ((y / self.cell_size) as usize).min(self.rows - 1);
        (cx, cy)
    }

    pub fn build(&mut self, xs: &[f32], ys: &[f32]) {
        let n = xs.len();
        self.cell_start.fill(0);
        self.items.resize(n, 0);
        self.sorted_x.resize(n, 0.0);
        self.sorted_y.resize(n, 0.0);

        // 1. 各セルの個体数を数える（cell_start[c + 1] に入れておく）
        for i in 0..n {
            let (cx, cy) = self.cell_xy(xs[i], ys[i]);
            self.cell_start[cy * self.cols + cx + 1] += 1;
        }
        // 2. 累積和を取って、各セルの開始位置にする
        for c in 1..self.cell_start.len() {
            self.cell_start[c] += self.cell_start[c - 1];
        }
        // 3. 個体番号を各セルの区画に書き込む
        self.cursor.copy_from_slice(&self.cell_start[..self.cols * self.rows]);
        for i in 0..n {
            let (cx, cy) = self.cell_xy(xs[i], ys[i]);
            let c = cy * self.cols + cx;
            let slot = self.cursor[c] as usize;
            self.items[slot] = i as u32;
            self.sorted_x[slot] = xs[i];
            self.sorted_y[slot] = ys[i];
            self.cursor[c] += 1;
        }
    }

    /// (x, y) を含むセルと周囲 8 セルに入っている個体を、(個体番号, x, y) として順に `f` に渡す。
    /// 座標は build() した時点のもの。距離の判定はしないので、呼び出し側で行う。
    pub fn for_each_nearby(&self, x: f32, y: f32, mut f: impl FnMut(usize, f32, f32)) {
        let (cx, cy) = self.cell_xy(x, y);
        // 端では反対側に折り返す。剰余（%）は割り算で遅いので、条件分岐で求める
        let rows = [
            if cy == 0 { self.rows - 1 } else { cy - 1 },
            cy,
            if cy + 1 == self.rows { 0 } else { cy + 1 },
        ];
        let cols = [
            if cx == 0 { self.cols - 1 } else { cx - 1 },
            cx,
            if cx + 1 == self.cols { 0 } else { cx + 1 },
        ];
        let wraps_x = cx == 0 || cx + 1 == self.cols;
        for row in rows {
            let base = row * self.cols;
            if wraps_x {
                // 左右の端: 3 セルがメモリ上で離れているので 1 セルずつ
                for col in cols {
                    self.visit_span(base + col, base + col + 1, &mut f);
                }
            } else {
                // 同じ行で横に並ぶ 3 セルは items 上で連続しているので、まとめて 1 区間で回す
                self.visit_span(base + cx - 1, base + cx + 2, &mut f);
            }
        }
    }

    /// セル first_cell から end_cell の手前までに入っている個体を順に f に渡す
    #[inline(always)]
    fn visit_span(&self, first_cell: usize, end_cell: usize, f: &mut impl FnMut(usize, f32, f32)) {
        let start = self.cell_start[first_cell] as usize;
        let end = self.cell_start[end_cell] as usize;
        for k in start..end {
            f(self.items[k] as usize, self.sorted_x[k], self.sorted_y[k]);
        }
    }

    /// セル順で k 番目の個体を (個体番号, x, y) で返す。
    /// 全個体を処理するループはこの順で回すと、近くの個体が続けて処理され、
    /// 近傍探索で触るメモリが前回とほぼ同じになるので速い。
    pub fn entry(&self, k: usize) -> (usize, f32, f32) {
        (self.items[k] as usize, self.sorted_x[k], self.sorted_y[k])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::world::torus_delta;

    #[test]
    fn 半径内の個体を総当たりと同じだけ見つける() {
        let (w, h, r) = (320.0, 240.0, 16.0);
        let mut rng = Rng::new(3);
        let xs: Vec<f32> = (0..2000).map(|_| rng.range(0.0, w)).collect();
        let ys: Vec<f32> = (0..2000).map(|_| rng.range(0.0, h)).collect();
        let mut grid = Grid::new(w, h, r);
        grid.build(&xs, &ys);
        for i in 0..xs.len() {
            let within = |xj: f32, yj: f32| {
                let dx = torus_delta(xj - xs[i], w);
                let dy = torus_delta(yj - ys[i], h);
                dx * dx + dy * dy < r * r
            };
            let mut found = 0;
            grid.for_each_nearby(xs[i], ys[i], |_, xj, yj| found += within(xj, yj) as u32);
            let expected = (0..xs.len()).filter(|&j| within(xs[j], ys[j])).count() as u32;
            assert_eq!(found, expected, "個体 {i}");
        }
    }
}

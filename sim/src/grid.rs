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
    /// 同じくセル順にした、個体ごとの付加情報（種類と、足元の草の量）。
    /// 近傍探索で相手の情報を読むとき、メモリ上の離れた場所を読みに行かずに済む
    sorted_kind: Vec<u8>,
    sorted_value: Vec<f32>,
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
            sorted_kind: Vec::new(),
            sorted_value: Vec::new(),
            cursor: vec![0; cols * rows],
        }
    }

    fn cell_xy(&self, x: f32, y: f32) -> (usize, usize) {
        let cx = ((x / self.cell_size) as usize).min(self.cols - 1);
        let cy = ((y / self.cell_size) as usize).min(self.rows - 1);
        (cx, cy)
    }

    /// kinds と values は個体ごとの付加情報。近傍探索のときに一緒に渡される
    pub fn build(&mut self, xs: &[f32], ys: &[f32], kinds: &[u8], values: &[f32]) {
        let n = xs.len();
        self.sorted_kind.resize(n, 0);
        self.sorted_value.resize(n, 0.0);
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
            self.sorted_kind[slot] = kinds[i];
            self.sorted_value[slot] = values[i];
            self.cursor[c] += 1;
        }
    }

    /// (x, y) から radius 以内に入りうるセルの個体を、(個体番号, x, y, 種類, 付加値) として順に `f` に渡す。
    /// セルを細かくしておけば、視野の小さい個体は狭い範囲だけ調べればよい。
    /// 距離の判定はしないので、呼び出し側で行う。
    pub fn for_each_within(&self, x: f32, y: f32, radius: f32, mut f: impl FnMut(usize, f32, f32, u8, f32)) {
        let (cx, cy) = self.cell_xy(x, y);
        // 調べるセルの半径。世界を一周して同じセルを二度数えないよう、列数・行数の半分未満に抑える
        let r = ((radius / self.cell_size).ceil() as usize)
            .min((self.cols - 1) / 2)
            .min((self.rows - 1) / 2);
        let wraps_x = cx < r || cx + r >= self.cols;
        for dy in 0..=2 * r {
            // 端では反対側に折り返す（cy + rows - r + dy を rows で割った余り。剰余は遅いので分岐で）
            let mut row = cy + self.rows + dy - r;
            if row >= self.rows {
                row -= self.rows;
            }
            if row >= self.rows {
                row -= self.rows;
            }
            let base = row * self.cols;
            if wraps_x {
                // 左右の端: 範囲が折り返してメモリ上で離れるので 1 セルずつ
                for dx in 0..=2 * r {
                    let mut col = cx + self.cols + dx - r;
                    if col >= self.cols {
                        col -= self.cols;
                    }
                    if col >= self.cols {
                        col -= self.cols;
                    }
                    self.visit_span(base + col, base + col + 1, &mut f);
                }
            } else {
                // 同じ行で横に並ぶセルは items 上で連続しているので、まとめて 1 区間で回す
                self.visit_span(base + cx - r, base + cx + r + 1, &mut f);
            }
        }
    }

    /// セル first_cell から end_cell の手前までに入っている個体を順に f に渡す
    #[inline(always)]
    fn visit_span(&self, first_cell: usize, end_cell: usize, f: &mut impl FnMut(usize, f32, f32, u8, f32)) {
        let start = self.cell_start[first_cell] as usize;
        let end = self.cell_start[end_cell] as usize;
        for k in start..end {
            f(self.items[k] as usize, self.sorted_x[k], self.sorted_y[k], self.sorted_kind[k], self.sorted_value[k]);
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
        let (w, h) = (320.0, 240.0);
        let mut rng = Rng::new(3);
        let xs: Vec<f32> = (0..2000).map(|_| rng.range(0.0, w)).collect();
        let ys: Vec<f32> = (0..2000).map(|_| rng.range(0.0, h)).collect();
        let mut grid = Grid::new(w, h, 8.0);
        grid.build(&xs, &ys, &vec![0; xs.len()], &vec![0.0; xs.len()]);
        // セルより小さい半径・何セルにもまたがる半径・半端な半径を試す（世界の端の折り返しも含む）
        for (i, r) in (0..xs.len()).map(|i| (i, [5.0, 16.0, 30.0, 47.5][i % 4])) {
            let within = |xj: f32, yj: f32| {
                let dx = torus_delta(xj - xs[i], w);
                let dy = torus_delta(yj - ys[i], h);
                dx * dx + dy * dy < r * r
            };
            let mut found = 0;
            grid.for_each_within(xs[i], ys[i], r, |_, xj, yj, _, _| found += within(xj, yj) as u32);
            let expected = (0..xs.len()).filter(|&j| within(xs[j], ys[j])).count() as u32;
            assert_eq!(found, expected, "個体 {i}（半径 {r}）");
        }
    }
}

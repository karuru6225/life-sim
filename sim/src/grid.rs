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
            self.items[self.cursor[c] as usize] = i as u32;
            self.cursor[c] += 1;
        }
    }

    /// (x, y) を含むセルと周囲 8 セルに入っている個体番号を順に `f` に渡す。
    /// 距離の判定はしないので、呼び出し側で行う。
    pub fn for_each_nearby(&self, x: f32, y: f32, mut f: impl FnMut(usize)) {
        let (cx, cy) = self.cell_xy(x, y);
        for dy in [self.rows - 1, 0, 1] {
            let row = (cy + dy) % self.rows;
            for dx in [self.cols - 1, 0, 1] {
                let col = (cx + dx) % self.cols;
                let c = row * self.cols + col;
                let start = self.cell_start[c] as usize;
                let end = self.cell_start[c + 1] as usize;
                for &j in &self.items[start..end] {
                    f(j as usize);
                }
            }
        }
    }
}

//! 長期の統計の記録。HISTORY_EVERY tick ごとに、種類ごとの形質の平均・出生と死因ごとの死亡の数・大人の大きさの分布を記録する。
//!
//! 記録が HISTORY_MAX を超えたら隣どうしをまとめて半分にし、以後は 2 倍の数の標本を 1 つにまとめて足す。
//! こうすると、どれだけ長く回しても数が一定以下で、全期間が同じ間隔で並ぶ。
//! 世界の進み方には関わらない（乱数を使わず、読むだけ）。保存ファイルにも入るので、画面なしで回した世界の推移もブラウザで見られる。

use super::{CARNIVORE, HERBIVORE, SIZE_HIST_BINS, World};

/// 記録する間隔（tick）
pub const HISTORY_EVERY: u64 = 200;
/// 記録の数の上限
pub const HISTORY_MAX: usize = 600;

/// 種類ごとの値の並び。0〜9 は形質の平均、10 は生まれた数、11〜18 は死因ごとの死亡数（死因の番号 0〜7）、19〜 は大きさの分布
pub mod field {
    pub const COUNT: usize = 0;
    pub const SIZE: usize = 1;
    pub const SPEED: usize = 2;
    pub const VISION: usize = 3;
    pub const FOV_DEG: usize = 4;
    pub const TEMP_PREF: usize = 5;
    pub const INVEST: usize = 6;
    pub const LITTER: usize = 7;
    pub const GENERATION: usize = 8;
    pub const BRAIN: usize = 9;
    pub const BIRTHS: usize = 10;
    pub const DEATHS: usize = 11;
    pub const SIZE_HIST: usize = 19;
}
/// 種類ごとの値の数
pub const KIND_LEN: usize = field::SIZE_HIST + SIZE_HIST_BINS;

/// 記録 1 つ。区間 (tick − span, tick] を表す。値は [草食, 肉食]
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub tick: u64,
    pub span: u64,
    pub values: [[f32; KIND_LEN]; 2],
}

impl Record {
    /// 2 つの記録をまとめる。形質と分布は区間の長さで重みをつけた平均、出生と死亡は合計
    fn merge(&self, later: &Record) -> Record {
        let span = self.span + later.span;
        let (wa, wb) = (self.span as f32 / span as f32, later.span as f32 / span as f32);
        let mut values = self.values;
        for (k, v) in values.iter_mut().enumerate() {
            for (f, x) in v.iter_mut().enumerate() {
                let y = later.values[k][f];
                *x = if (field::BIRTHS..field::SIZE_HIST).contains(&f) { *x + y } else { *x * wa + y * wb };
            }
        }
        Record { tick: later.tick, span, values }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub records: Vec<Record>,
    /// 記録 1 つにまとめる標本の数（1, 2, 4, …）
    pub stride: u32,
    /// まとめている途中の記録と、そこに入っている標本の数
    pub pending: Option<Record>,
    pub pending_count: u32,
    /// 前の標本の tick と、そのときの出生・死亡の累計 [種類]
    pub last_tick: u64,
    pub last_births: [u64; 2],
    pub last_deaths: [[u64; 8]; 2],
    /// 記録を並べ直した（まとめた）回数。JS が全部を取り直すかの判断に使う。保存しない
    pub version: u32,
}

impl History {
    pub fn new() -> Self {
        History { stride: 1, ..Default::default() }
    }

    fn add(&mut self, r: Record) {
        let merged = match self.pending.take() {
            Some(p) => p.merge(&r),
            None => r,
        };
        self.pending_count += 1;
        if self.pending_count < self.stride.max(1) {
            self.pending = Some(merged);
            return;
        }
        self.records.push(merged);
        self.pending_count = 0;
        if self.records.len() > HISTORY_MAX {
            let halved: Vec<Record> = self
                .records
                .chunks(2)
                .map(|c| if c.len() == 2 { c[0].merge(&c[1]) } else { c[0].clone() })
                .collect();
            self.records = halved;
            self.stride = self.stride.max(1) * 2;
            self.version += 1;
        }
    }

    /// 表示に使う記録の数（まとめ途中のものを含む）
    pub fn len(&self) -> usize {
        self.records.len() + self.pending.is_some() as usize
    }

    /// i 番目の記録（最後はまとめ途中のもの）
    pub fn get(&self, i: usize) -> Option<&Record> {
        self.records.get(i).or_else(|| if i == self.records.len() { self.pending.as_ref() } else { None })
    }
}

impl World {
    /// 前の標本から HISTORY_EVERY tick 以上進んでいたら、標本を 1 つ取って記録に足す（1 tick の終わりに呼ぶ）
    pub(super) fn sample_history(&mut self) {
        if self.tick < self.history.last_tick + HISTORY_EVERY {
            return;
        }
        let mut values = [[0.0f32; KIND_LEN]; 2];
        for (k, kind) in [HERBIVORE, CARNIVORE].into_iter().enumerate() {
            let v = &mut values[k];
            // 形質の平均は、系統の統計を「全部の色相」で取って得る
            let l = self.lineage_stats(kind, 0.0, 1.0);
            v[field::COUNT] = l[0] as f32;
            v[field::SIZE] = l[2] as f32;
            v[field::SPEED] = l[3] as f32;
            v[field::VISION] = l[4] as f32;
            v[field::FOV_DEG] = l[5] as f32;
            v[field::TEMP_PREF] = l[6] as f32;
            v[field::INVEST] = l[7] as f32;
            v[field::GENERATION] = l[9] as f32;
            v[field::BRAIN] = (l[15] + l[16]) as f32;
            v[field::LITTER] = l[17] as f32;
            let births = self.census.births[k];
            v[field::BIRTHS] = births.saturating_sub(self.history.last_births[k]) as f32;
            self.history.last_births[k] = births;
            for cause in 0..8 {
                let d = self.census.deaths[k][cause];
                v[field::DEATHS + cause] = d.saturating_sub(self.history.last_deaths[k][cause]) as f32;
                self.history.last_deaths[k][cause] = d;
            }
            for (b, &n) in self.adult_size_histogram(kind).iter().enumerate() {
                v[field::SIZE_HIST + b] = n as f32;
            }
        }
        let span = self.tick - self.history.last_tick;
        self.history.last_tick = self.tick;
        self.history.add(Record { tick: self.tick, span, values });
    }

    /// 記録の基準を今の状態にする（履歴のない保存ファイルを読み込んだとき）
    pub(super) fn rebase_history(&mut self) {
        self.history = History::new();
        self.history.last_tick = self.tick;
        self.history.last_births = self.census.births;
        self.history.last_deaths = self.census.deaths;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tick: u64, span: u64, births: f32, size: f32) -> Record {
        let mut values = [[0.0; KIND_LEN]; 2];
        values[0][field::BIRTHS] = births;
        values[0][field::SIZE] = size;
        Record { tick, span, values }
    }

    #[test]
    fn まとめると出生は合計し形質は区間の長さで平均する() {
        let m = record(200, 200, 3.0, 1.0).merge(&record(800, 600, 5.0, 2.0));
        assert_eq!((m.tick, m.span), (800, 800));
        assert_eq!(m.values[0][field::BIRTHS], 8.0);
        assert!((m.values[0][field::SIZE] - 1.75).abs() < 1e-6);
    }

    #[test]
    fn 記録が上限を超えたら半分にまとめ以後は倍の標本を1つにする() {
        let mut h = History::new();
        for i in 1..=(HISTORY_MAX as u64 + 1) {
            h.add(record(i * 200, 200, 1.0, 1.0));
        }
        assert_eq!(h.records.len(), (HISTORY_MAX + 1).div_ceil(2));
        assert_eq!(h.stride, 2);
        assert_eq!(h.version, 1);
        let before = h.records.len();
        h.add(record(1_000_000, 200, 1.0, 1.0));
        assert_eq!(h.records.len(), before, "1 つ目はまとめ途中");
        assert_eq!(h.len(), before + 1);
        h.add(record(1_000_200, 200, 1.0, 1.0));
        assert_eq!(h.records.len(), before + 1);
        assert!(h.pending.is_none());
        let total: f32 = h.records.iter().map(|r| r.values[0][field::BIRTHS]).sum();
        assert_eq!(total, HISTORY_MAX as f32 + 3.0, "まとめても出生の合計は変わらない");
    }
}

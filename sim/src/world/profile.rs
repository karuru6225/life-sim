//! 1 tick の段ごとの時間の計測（profile 機能のときだけ。std::time は WASM で使えないので、ネイティブの画面なし実行用）。
//! 例: cargo run --release --features parallel,profile --example headless --manifest-path sim/Cargo.toml -- 10000

use std::sync::Mutex;
use std::time::Instant;

/// 段の名前（World::step の順）
pub const NAMES: [&str; 9] = ["気候・植物の成長", "死骸・隠れ", "遠くの景色", "マス目・呼び声", "判断（知覚と脳）", "行動（食べる・狩る・争う・相手探し）", "移動", "代謝・繁殖", "出生・死亡・記録"];

static TOTALS: Mutex<[f64; NAMES.len()]> = Mutex::new([0.0; NAMES.len()]);

/// 段の区切りまでの時間を足していく
pub struct Laps(Instant);

impl Laps {
    pub fn start() -> Self {
        Laps(Instant::now())
    }

    /// 前の区切りからここまでを段 k の時間として足す
    pub fn lap(&mut self, k: usize) {
        let now = Instant::now();
        TOTALS.lock().unwrap()[k] += (now - self.0).as_secs_f64();
        self.0 = now;
    }

    /// 区切りを進めずに、段 k に secs 秒を足す（段の中をさらに分けて測った分）
    pub fn add(k: usize, secs: f64) {
        TOTALS.lock().unwrap()[k] += secs;
    }

    /// 前の区切りを今にする（add で足した分を二重に数えないため）
    pub fn reset(&mut self) {
        self.0 = Instant::now();
    }
}

/// 段ごとの合計時間（秒）
pub fn totals() -> [f64; NAMES.len()] {
    *TOTALS.lock().unwrap()
}

/// 判断の段（plan）の中の区切りの名前
pub const PLAN_NAMES: [&str; 4] = [
    "知覚（視野の中の個体を探す）",
    "草の量と方向・遠くの景色",
    "その他の入力（呼び声・死体・群れなど）",
    "脳",
];

/// 判断の段の中の区切りごとの合計時間（ナノ秒）。並列に足すので原子的に足す
static PLAN_NS: [std::sync::atomic::AtomicU64; PLAN_NAMES.len()] = [const { std::sync::atomic::AtomicU64::new(0) }; PLAN_NAMES.len()];

/// 判断の段の中の区切りまでの時間を足していく（個体ごとに作る）
pub struct Stopwatch(Instant);

impl Stopwatch {
    pub fn start() -> Self {
        Stopwatch(Instant::now())
    }

    /// 前の区切りからここまでを区切り k の時間として足す
    pub fn lap(&mut self, k: usize) {
        let now = Instant::now();
        PLAN_NS[k].fetch_add((now - self.0).as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        self.0 = now;
    }
}

/// 判断の段の中の区切りごとの合計時間（秒。並列のときはスレッドの合計）
pub fn plan_totals() -> [f64; PLAN_NAMES.len()] {
    std::array::from_fn(|k| PLAN_NS[k].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9)
}

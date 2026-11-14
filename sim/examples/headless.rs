//! 画面なしでシミュレーションを回し、個体数・形質・群れの度合いの推移を表示する。バランス調整用。
//!
//! 使い方:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- [tick数] [名前=値 ...]
//! 例:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- 30000 sunlight=0.1 seed=3 every=1000
//!
//! 列の意味:
//!   集まり: 集まり具合（1 でランダム、小さいほど集まっている）
//!   そろい: 近くの仲間との向きのそろい具合（0 でばらばら、1 で同じ向き）
//!   同系/他系: そろい具合を、同じ系統どうし・違う系統どうしに分けたもの
//!   同系率: 近くの仲間のうち同じ系統の割合
//!   失敗: 直前の区間で、交配が相性で失敗した割合

use sim::params::Params;
use sim::world::{CARNIVORE, HERBIVORE, World};
use std::time::Instant;

fn main() {
    let mut ticks: u64 = 20_000;
    let mut report_every: u64 = 1_000;
    let mut params = Params::default();
    let mut seed = 1;
    for arg in std::env::args().skip(1) {
        if let Some((name, value)) = arg.split_once('=') {
            let value: f32 = value.parse().expect("値が数値ではない");
            match name {
                "seed" => seed = value as u64,
                "every" => report_every = value as u64,
                _ if !params.set(name, value) => panic!("知らないパラメータ: {name}"),
                _ => {}
            }
        } else {
            ticks = arg.parse().expect("tick 数が数値ではない");
        }
    }

    let mut w = World::new(seed, params);
    println!(
        "{:>7} {:>6} {:>5} {:>7} | 草食: {:>5} {:>5} {:>5} {:>5} {:>4} | {:>5} {:>5} {:>5} {:>5} {:>5} | 肉食: {:>5} {:>5} | {:>5} | {:>6}",
        "tick", "草食", "肉食", "植物量", "大きさ", "速度", "視野", "投資", "世代", "集まり", "そろい", "同系", "他系", "同系率", "大きさ", "速度", "失敗", "ms/tick"
    );
    let mut t = Instant::now();
    let (mut last_matings, mut last_failed) = (0u64, 0u64);
    for tick in 1..=ticks {
        w.step();
        if tick % report_every == 0 {
            let ms = t.elapsed().as_secs_f64() * 1000.0 / report_every as f64;
            let h = w.trait_means(HERBIVORE);
            let c = w.trait_means(CARNIVORE);
            let hh = w.herd_stats(HERBIVORE);
            let matings = w.matings - last_matings;
            let failed = w.failed_matings - last_failed;
            (last_matings, last_failed) = (w.matings, w.failed_matings);
            let fail = if matings > 0 { failed as f64 / matings as f64 } else { 0.0 };
            println!(
                "{tick:>7} {:>6} {:>5} {:>7.0} |       {:>5.2} {:>5.2} {:>5.1} {:>5.2} {:>4.0} | {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2} |       {:>5.2} {:>5.2} | {:>5.3} | {ms:>6.2}",
                h[0], c[0], w.plants.total(), h[1], h[2], h[3], h[4], h[5], hh[0], hh[1], hh[2], hh[3], hh[4], c[1], c[2], fail
            );
            if w.count_kind(HERBIVORE) == 0 && w.count_kind(CARNIVORE) == 0 {
                println!("全滅");
                break;
            }
            t = Instant::now();
        }
    }
    let err = w.ledger.expected_total() - w.total_energy();
    println!("エネルギー帳尻の誤差: {err:.4}（帳簿 {:.1}）", w.ledger.expected_total());
}

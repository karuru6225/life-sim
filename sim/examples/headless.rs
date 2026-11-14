//! 画面なしでシミュレーションを回し、個体数と形質の平均の推移を表示する。バランス調整用。
//!
//! 使い方:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- [tick数] [名前=値 ...]
//! 例:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- 30000 sunlight=0.1 seed=3

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
        "{:>7} {:>7} {:>6} {:>7} | 草食: {:>5} {:>5} {:>5} {:>5} {:>5} | 肉食: {:>5} {:>5} {:>5} {:>5} {:>5} | {:>6}",
        "tick", "草食", "肉食", "植物量", "大きさ", "速度", "視野", "投資", "世代", "大きさ", "速度", "視野", "投資", "世代", "ms/tick"
    );
    let mut t = Instant::now();
    for tick in 1..=ticks {
        w.step();
        if tick % report_every == 0 {
            let ms = t.elapsed().as_secs_f64() * 1000.0 / report_every as f64;
            let h = w.trait_means(HERBIVORE);
            let c = w.trait_means(CARNIVORE);
            println!(
                "{tick:>7} {:>7} {:>6} {:>7.0} |       {:>5.2} {:>5.2} {:>5.1} {:>5.2} {:>5.0} |       {:>5.2} {:>5.2} {:>5.1} {:>5.2} {:>5.0} | {ms:>6.2}",
                h[0], c[0], w.plants.total(), h[1], h[2], h[3], h[4], h[5], c[1], c[2], c[3], c[4], c[5]
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

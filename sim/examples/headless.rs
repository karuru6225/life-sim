//! 画面なしでシミュレーションを回し、個体数の推移を表示する。バランス調整用。
//!
//! 使い方:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- [tick数] [名前=値 ...]
//! 例:
//!   cargo run --release --example headless --manifest-path sim/Cargo.toml -- 30000 sunlight=0.04

use sim::params::Params;
use sim::world::{CARNIVORE, HERBIVORE, World};
use std::time::Instant;

fn main() {
    let mut ticks: u64 = 20_000;
    let mut params = Params::default();
    let mut seed = 1;
    for arg in std::env::args().skip(1) {
        if let Some((name, value)) = arg.split_once('=') {
            let value: f32 = value.parse().expect("値が数値ではない");
            if name == "seed" {
                seed = value as u64;
            } else if !params.set(name, value) {
                panic!("知らないパラメータ: {name}");
            }
        } else {
            ticks = arg.parse().expect("tick 数が数値ではない");
        }
    }

    let mut w = World::new(seed, params);
    println!("{:>7} {:>8} {:>7} {:>10} {:>8}", "tick", "草食", "肉食", "植物量", "ms/tick");
    let report_every = 600;
    let mut t = Instant::now();
    for tick in 1..=ticks {
        w.step();
        if tick % report_every == 0 {
            let ms = t.elapsed().as_secs_f64() * 1000.0 / report_every as f64;
            let herb = w.count_kind(HERBIVORE);
            let carn = w.count_kind(CARNIVORE);
            println!("{tick:>7} {herb:>8} {carn:>7} {:>10.0} {ms:>8.2}", w.plants.total());
            if herb == 0 && carn == 0 {
                println!("全滅");
                break;
            }
            t = Instant::now();
        }
    }
    let err = w.ledger.expected_total() - w.total_energy();
    println!("エネルギー帳尻の誤差: {err:.4}（帳簿 {:.1}）", w.ledger.expected_total());
}

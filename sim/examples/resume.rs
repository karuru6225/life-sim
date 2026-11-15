//! 保存ファイル（ブラウザで保存した .lifesim.gz は不可。画面なしの実行器で書き出した圧縮なしの .lifesim）から世界を読み込み、
//! 続きを画面なしで進める。必要なら、進めたあとを別のファイルに保存する。
//!
//! 使い方:
//!   cargo run --release --features parallel --example resume --manifest-path sim/Cargo.toml -- world.lifesim 10000 [save=続き.lifesim]

use sim::world::{CARNIVORE, HERBIVORE, World};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u64 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(1000);
    let save = args.find_map(|a| a.strip_prefix("save=").map(str::to_string));

    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let payload = strip_container(&data);
    let (mut w, unknown) = World::load(payload).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    if !unknown.is_empty() {
        println!("知らないパラメータを無視した: {unknown:?}");
    }
    println!("tick {} から再開（草食 {}、肉食 {}）", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));
    let t = Instant::now();
    let every = (ticks / 10).max(1);
    for k in 1..=ticks {
        w.step();
        if k % every == 0 {
            println!(
                "tick {:>7}  草食 {:>6}  肉食 {:>5}  {:.2} ms/tick",
                w.tick,
                w.count_kind(HERBIVORE),
                w.count_kind(CARNIVORE),
                t.elapsed().as_secs_f64() * 1000.0 / k as f64
            );
        }
    }
    let err = w.ledger.expected_total() - w.total_energy();
    println!("エネルギー帳尻の誤差: {err:.4}");
    if let Some(out) = save {
        std::fs::write(&out, sim::world::save_file(&w, "resume", "")).expect("保存ファイルを書けない");
        println!("{out} に保存した");
    }
}

/// 保存ファイルの入れ物（"LIFESIM\n"・見出しの長さ・見出し）を外して、世界のバイト列を返す
fn strip_container(data: &[u8]) -> &[u8] {
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = String::from_utf8_lossy(&data[12..12 + len]);
    println!("見出し: {header}");
    &data[12 + len..]
}

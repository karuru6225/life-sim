//! 保存ファイル（圧縮なしの .lifesim。ブラウザの .lifesim.gz は `gzip -dc` で展開してから）を読み込み、
//! 草食と肉食の大人の大きさの分布を、文字の棒グラフで表示する。世界は進めない。
//!
//! 使い方:
//!   cargo run --release --example size_hist --manifest-path sim/Cargo.toml -- world.lifesim [width=0.05]

use sim::world::{CARNIVORE, HERBIVORE, World};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let width: f32 = args.find_map(|a| a.strip_prefix("width=").map(|v| v.parse().expect("幅が数値ではない"))).unwrap_or(0.05);

    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let payload = strip_container(&data);
    let (w, _) = World::load(payload).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));
    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let sizes: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.traits[i].adult_size).collect();
        histogram(name, &sizes, width);
    }
}

fn histogram(name: &str, sizes: &[f32], width: f32) {
    if sizes.is_empty() {
        println!("\n{name}: いない");
        return;
    }
    let mut sorted = sizes.to_vec();
    sorted.sort_by(f32::total_cmp);
    let q = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let mean = sorted.iter().map(|&s| s as f64).sum::<f64>() / sorted.len() as f64;
    println!(
        "\n{name}の大人の大きさ（{} 体）  平均 {mean:.3}  最小 {:.3}  10% {:.3}  中央 {:.3}  90% {:.3}  最大 {:.3}",
        sorted.len(),
        q(0.0),
        q(0.1),
        q(0.5),
        q(0.9),
        q(1.0)
    );
    let lo = (q(0.0) / width).floor() as i32;
    let hi = (q(1.0) / width).floor() as i32;
    let mut counts = vec![0usize; (hi - lo + 1) as usize];
    for &s in &sorted {
        counts[((s / width).floor() as i32 - lo) as usize] += 1;
    }
    let peak = *counts.iter().max().unwrap();
    for (k, &n) in counts.iter().enumerate() {
        let from = (lo + k as i32) as f32 * width;
        let bar = "#".repeat((n * 50).div_ceil(peak.max(1)));
        println!("  {:>5.2}-{:<5.2} {:>6} {:>5.1}% {}", from, from + width, n, n as f64 * 100.0 / sorted.len() as f64, bar);
    }
}

/// 保存ファイルの中身（ヘッダーの後ろ）を取り出す。resume.rs と同じ
fn strip_container(data: &[u8]) -> &[u8] {
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = String::from_utf8_lossy(&data[12..12 + len]);
    println!("見出し: {header}");
    &data[12 + len..]
}

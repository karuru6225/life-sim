//! 保存ファイル（圧縮なしの .lifesim。ブラウザの .lifesim.gz は `gzip -dc` で展開してから）を読み込み、
//! 草食と肉食の大人の大きさの分布を、文字の棒グラフで表示する。世界は進めない。
//!
//! 使い方:
//!   cargo run --release --example size_hist --manifest-path sim/Cargo.toml -- world.lifesim [width=0.05]
//!
//! 草食は、今いる場所の林の度合いで 3 つ（草原 0〜1/3・中間・林 2/3〜1）に分けた分布も表示する（場所による住み分けの確認用）

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
    habitat_table(&w, width);
}

/// 草食の大きさの段階ごとに、今いる場所の林の度合い（草原・中間・林）の内訳を表示する
fn habitat_table(w: &World, width: f32) {
    let herbs: Vec<(f32, usize)> = (0..w.count())
        .filter(|&i| w.kind[i] == HERBIVORE)
        .map(|i| {
            let wood = w.plants.wood[w.plants.cell_index(w.xs[i], w.ys[i])];
            (w.traits[i].adult_size, ((wood * 3.0) as usize).min(2))
        })
        .collect();
    if herbs.is_empty() {
        return;
    }
    let mut totals = [0usize; 3];
    let mut sums = [0.0f64; 3];
    for &(s, h) in &herbs {
        totals[h] += 1;
        sums[h] += s as f64;
    }
    println!("
草食の今いる場所の内訳（草原 / 中間 / 林）  体数 {} / {} / {}  平均の大きさ {:.3} / {:.3} / {:.3}",
        totals[0], totals[1], totals[2],
        sums[0] / totals[0].max(1) as f64, sums[1] / totals[1].max(1) as f64, sums[2] / totals[2].max(1) as f64);
    let lo = herbs.iter().map(|h| (h.0 / width).floor() as i32).min().unwrap();
    let hi = herbs.iter().map(|h| (h.0 / width).floor() as i32).max().unwrap();
    let mut counts = vec![[0usize; 3]; (hi - lo + 1) as usize];
    for &(s, h) in &herbs {
        counts[((s / width).floor() as i32 - lo) as usize][h] += 1;
    }
    println!("  大きさ        草原   中間     林   （その場所の草食に占める割合。林にいる割合）");
    for (k, c) in counts.iter().enumerate() {
        let n: usize = c.iter().sum();
        if n == 0 {
            continue;
        }
        let from = (lo + k as i32) as f32 * width;
        let pct = |h: usize| c[h] as f64 * 100.0 / totals[h].max(1) as f64;
        println!("  {:>5.2}-{:<5.2} {:>5.1}% {:>5.1}% {:>5.1}%   林 {:>3.0}%", from, from + width, pct(0), pct(1), pct(2), c[2] as f64 * 100.0 / n as f64);
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

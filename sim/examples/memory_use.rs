//! 保存ファイル（圧縮なしの .lifesim）を読み込み、脳が記憶の入力を使っているかを測る。世界は進めない。
//!
//! 個体ごとに、今の入力のまま脳を動かした出力と、記憶の入力（覚えた場所の方向と良さ）だけ 0 にした出力を比べる。
//! 比べるのは、進む向きに効く「自由な移動の方向 × その優先度」と、食べる欲求の優先度・速さ。
//! 比較のため、遠くの景色の入力（方向）を 0 にした場合も同じように測る。
//!
//! 使い方:
//!   cargo run --release --example memory_use --manifest-path sim/Cargo.toml -- world.lifesim

use sim::genome::{self, N_IN, N_OUT, softsign};
use sim::world::{CARNIVORE, HERBIVORE, World};

fn main() {
    let path = std::env::args().nth(1).expect("保存ファイルを指定する");
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let payload = strip_container(&data);
    let (mut w, _) = World::load(payload).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));
    let inputs = w.brain_inputs();

    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let members: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == kind).collect();
        if members.is_empty() {
            continue;
        }
        let n = members.len() as f32;
        let memory: Vec<f32> = members.iter().map(|&i| w.traits[i].memory).collect();
        let mut sorted = memory.clone();
        sorted.sort_by(f32::total_cmp);
        let q = |p: f32| sorted[((sorted.len() - 1) as f32 * p) as usize];
        let remembers = members.iter().filter(|&&i| inputs[i][genome::IN_MEM_VALUE] > 0.0).count();
        let pointing = members
            .iter()
            .filter(|&&i| inputs[i][genome::IN_MEM_FWD] != 0.0 || inputs[i][genome::IN_MEM_RIGHT] != 0.0)
            .count();
        println!("\n{name}（{} 体）", members.len());
        println!(
            "  記憶の長さ: 平均 {:.0} tick、10% {:.0} / 50% {:.0} / 90% {:.0}",
            memory.iter().sum::<f32>() / n,
            q(0.1),
            q(0.5),
            q(0.9)
        );
        println!(
            "  何かを覚えている {:.0}%、覚えた場所が離れていて方向が出ている {:.0}%",
            remembers as f32 / n * 100.0,
            pointing as f32 / n * 100.0
        );
        println!("  入力を 0 にしたときの出力の変化（方向が出ている個体の平均 / 変化が 0.05 を超える個体の割合）:");
        println!("    {:<16} {:>16} {:>16} {:>16}", "", "自由な移動の向き", "食べる優先度", "速さ");
        for (label, zero) in [
            ("記憶", &[genome::IN_MEM_FWD, genome::IN_MEM_RIGHT, genome::IN_MEM_VALUE][..]),
            ("遠くの景色", &[genome::IN_FAR_FWD, genome::IN_FAR_RIGHT][..]),
        ] {
            let active: Vec<usize> = members.iter().copied().filter(|&i| zero.iter().any(|&k| inputs[i][k] != 0.0)).collect();
            if active.is_empty() {
                println!("    {label:<16} （入力が出ている個体がいない）");
                continue;
            }
            let mut sums = [0.0f32; 3];
            let mut over = [0usize; 3];
            for &i in &active {
                let full = genome::think(w.genome(i), &inputs[i]);
                let mut cut = inputs[i];
                for &k in zero {
                    cut[k] = 0.0;
                }
                let without = genome::think(w.genome(i), &cut);
                let d = effects(&full, &without);
                for k in 0..3 {
                    sums[k] += d[k];
                    over[k] += (d[k] > 0.05) as usize;
                }
            }
            let m = active.len() as f32;
            let cell = |k: usize| format!("{:.3} / {:>3.0}%", sums[k] / m, over[k] as f32 / m * 100.0);
            println!("    {label:<16} {:>16} {:>16} {:>16}   （{} 体）", cell(0), cell(1), cell(2), active.len());
        }
    }
}

/// 2 つの出力の違い: [自由な移動の向き × 優先度 の差の大きさ, 食べる欲求の優先度の差, 速さの差]
fn effects(a: &[f32; N_OUT], b: &[f32; N_OUT]) -> [f32; 3] {
    let free = |o: &[f32; N_OUT]| {
        let p = genome::priorities(o)[genome::N_DRIVES - 1];
        (p * softsign(o[genome::OUT_DIR_FWD]), p * softsign(o[genome::OUT_DIR_RIGHT]))
    };
    let (fa, fb) = (free(a), free(b));
    let throttle = |o: &[f32; N_OUT]| 0.5 + 0.5 * softsign(o[genome::OUT_THROTTLE]);
    [
        ((fa.0 - fb.0).powi(2) + (fa.1 - fb.1).powi(2)).sqrt(),
        (genome::priorities(a)[0] - genome::priorities(b)[0]).abs(),
        (throttle(a) - throttle(b)).abs(),
    ]
}

const _: () = assert!(N_IN > genome::IN_MEM_VALUE);

fn strip_container(data: &[u8]) -> &[u8] {
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = String::from_utf8_lossy(&data[12..12 + len]);
    println!("見出し: {header}");
    &data[12 + len..]
}

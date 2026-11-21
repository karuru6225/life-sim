//! 保存ファイル（圧縮なしの .lifesim）を読み込み、巣の損得の大きさを測る（巣作りが進化しない理由を調べる）。
//!
//! - 生きる費用の内訳（世界を少し進めて数える）と、そのうち体温の費用の割合
//! - 今いる場所で出来上がった巣（出来の良さ 1）の中にいたら、1 tick にどれだけ体温の費用が減るか。巣を作る費用と比べる
//! - 足元の草の隠れ（巣がなくても隠れられる度合い）。巣の隠れ（0.9 × 出来の良さ）がこれを上回る個体の割合
//! - 生まれたときの成長度と、動けない子（成長度が immobile_growth 未満）の割合
//! - 出力「巣を作る」の値の分布（正で作る）
//!
//! 使い方:
//!   cargo run --release --example nest_why --manifest-path sim/Cargo.toml -- world.lifesim [tick 数（既定 300）]

use sim::genome::{self, OUT_BUILD};
use sim::world::{CARNIVORE, COST_NAMES, HERBIVORE, World};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u32 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(300);
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let (mut w, _) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));

    // 今の世界での、巣の得の見積もり
    let inputs = w.brain_inputs();
    let p = w.params.clone();
    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let idx: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == kind).collect();
        let m = idx.len().max(1) as f32;
        let mut temp_diff = 0.0;
        let mut thermal = 0.0;
        let mut save = 0.0;
        let mut build_cost = 0.0;
        let mut cover = 0.0;
        let mut nest_better = 0.0;
        let mut out_build = Vec::new();
        for &i in &idx {
            let t = &w.traits[i];
            let d = (w.terrain.temperature_at(w.xs[i], w.ys[i]) - t.temp_pref).abs();
            let fat = (w.energy[i] / w.energy_cap(i).max(1e-6)).clamp(0.0, 1.0);
            let th = p.thermo_cost * d * t.thermal_scale * (1.0 - p.fat_insulation * fat);
            temp_diff += d;
            thermal += th;
            save += p.nest_insulation * th;
            // 出来の良さ 0 から 1 まで作る費用（作る tick 数 × 1 tick の費用）
            let build_ticks = p.nest_material_need / (p.nest_material * t.size.max(1e-3));
            build_cost += build_ticks * p.nest_build_cost * t.size_pow075;
            cover += w.cover[i];
            nest_better += (p.nest_hide > w.cover[i]) as u8 as f32;
            out_build.push(genome::think(w.genome(i), &inputs[i])[OUT_BUILD]);
        }
        out_build.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |f: f32| out_build.get(((out_build.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(0.0);
        println!("\n{name}（{} 体）", idx.len());
        println!("  好みの気温との差 {:.1} 度、体温の費用 {:.5}/tick", temp_diff / m, thermal / m);
        println!(
            "  出来上がった巣の中にいれば減る体温の費用 {:.5}/tick。巣を 0 から作る費用 {:.3}（巣の中に {:.0} tick いて取り戻せる）",
            save / m,
            build_cost / m,
            build_cost / save.max(1e-9)
        );
        println!(
            "  足元の草の隠れ {:.2}。巣の隠れ（{:.1}）のほうが大きい個体 {:.0}%",
            cover / m,
            p.nest_hide,
            nest_better / m * 100.0
        );
        println!(
            "  出力「巣を作る」: 最小 {:+.2} / 10% {:+.2} / 中央 {:+.2} / 90% {:+.2} / 最大 {:+.2}。正の個体 {:.2}%",
            q(0.0),
            q(0.1),
            q(0.5),
            q(0.9),
            q(1.0),
            out_build.iter().filter(|&&v| v > 0.0).count() as f32 / m * 100.0
        );
    }

    // 世界を進めて、生きる費用の内訳と生まれた子を数える
    let deaths0 = w.census.deaths;
    let births0 = w.census.births;
    for _ in 0..ticks {
        w.step();
    }
    println!("\n{ticks} tick 進めたあいだ");
    for (k, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let k = k as usize;
        let c = w.census.costs[k];
        let total: f64 = c.iter().sum();
        let parts: Vec<String> =
            COST_NAMES.iter().zip(c).map(|(n, v)| format!("{n} {:.1}%", v / total.max(1e-12) * 100.0)).collect();
        let births = w.census.births[k] - births0[k];
        let deaths: Vec<u64> = (0..8).map(|c| w.census.deaths[k][c] - deaths0[k][c]).collect();
        let dead: u64 = deaths.iter().sum();
        println!("\n{name}: 生きる費用の内訳 {}", parts.join("、"));
        println!(
            "  生まれた {births} 体、生まれたときの成長度の平均 {:.2}。子に与えた回数 {}",
            w.census.birth_growth[k] / births.max(1) as f64,
            w.census.feeds[k]
        );
        println!(
            "  死んだ {dead} 体: 餓死 {:.0}% / 老衰 {:.0}% / 食べられた {:.0}%",
            deaths[1] as f64 / dead.max(1) as f64 * 100.0,
            deaths[2] as f64 / dead.max(1) as f64 * 100.0,
            deaths[3] as f64 / dead.max(1) as f64 * 100.0
        );
        let young: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] as usize == k && w.growth[i] < 1.0).collect();
        let immobile = young.iter().filter(|&&i| w.growth[i] < p.immobile_growth).count();
        println!("  今の幼体 {} 体のうち動けない（成長度 {} 未満）{} 体", young.len(), p.immobile_growth, immobile);
    }
}

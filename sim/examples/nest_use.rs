//! 保存ファイル（圧縮なしの .lifesim）を読み込み、巣を誰が・どこに作り、使っているかを数える。世界は進めない。
//!
//! - 種類ごと: 巣を覚えている個体・巣の中にいる個体・巣を作りたがる個体（今の入力で出力「巣を作る」が正）の割合
//! - 草食の大人の大きさの段階ごと: 巣を覚えている割合（小さい草食に巣作りが偏るか）
//! - 巣のあるマス（出来の良さ 0.1 超）と陸全体の、林の度合いと平均気温の比較（林・寒い地域に偏るか）
//! - 巣の持ち主の種類
//!
//! 使い方:
//!   cargo run --release --example nest_use --manifest-path sim/Cargo.toml -- world.lifesim

use sim::genome::{self, OUT_BUILD};
use sim::world::{CARNIVORE, HERBIVORE, NO_HOME, World};

fn main() {
    let path = std::env::args().nth(1).expect("保存ファイルを指定する");
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let payload = strip_container(&data);
    let (mut w, _) = World::load(payload).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));
    let inputs = w.brain_inputs();
    let in_nest = |w: &World, i: usize| w.nest[w.plants.cell_index(w.xs[i], w.ys[i])] > 0.1;

    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let members: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == kind).collect();
        if members.is_empty() {
            continue;
        }
        let n = members.len() as f32;
        let homed = members.iter().filter(|&&i| w.home[i] != NO_HOME).count() as f32;
        let inside = members.iter().filter(|&&i| in_nest(&w, i)).count() as f32;
        let wants = members.iter().filter(|&&i| genome::think(w.genome(i), &inputs[i])[OUT_BUILD] > 0.0).count() as f32;
        let juveniles: Vec<usize> = members.iter().copied().filter(|&i| w.growth[i] < 1.0).collect();
        let juv_inside = juveniles.iter().filter(|&&i| in_nest(&w, i)).count() as f32;
        println!("\n{name}（{} 体）", members.len());
        println!(
            "  巣を覚えている {:.0}%、巣の中にいる {:.0}%（幼体 {:.0}%、{} 体）、今の入力で巣を作りたがる {:.1}%",
            homed / n * 100.0,
            inside / n * 100.0,
            juv_inside / juveniles.len().max(1) as f32 * 100.0,
            juveniles.len(),
            wants / n * 100.0
        );
    }

    // 草食の大人の大きさの段階ごとの、巣を覚えている割合
    println!("\n草食の大人の大きさごと（巣を覚えている / 巣の中にいる / 作りたがる）");
    let edges = [0.0f32, 0.5, 0.7, 0.9, 1.1, 1.3, f32::INFINITY];
    for k in 0..edges.len() - 1 {
        let members: Vec<usize> = (0..w.count())
            .filter(|&i| w.kind[i] == HERBIVORE && w.traits[i].adult_size >= edges[k] && w.traits[i].adult_size < edges[k + 1])
            .collect();
        if members.is_empty() {
            continue;
        }
        let n = members.len() as f32;
        let homed = members.iter().filter(|&&i| w.home[i] != NO_HOME).count() as f32;
        let inside = members.iter().filter(|&&i| in_nest(&w, i)).count() as f32;
        let wants = members.iter().filter(|&&i| genome::think(w.genome(i), &inputs[i])[OUT_BUILD] > 0.0).count() as f32;
        println!(
            "  {:>4.1}〜{:<4.1} {:>6} 体: {:>5.1}% / {:>5.1}% / {:>5.1}%",
            edges[k],
            edges[k + 1].min(9.9),
            members.len(),
            homed / n * 100.0,
            inside / n * 100.0,
            wants / n * 100.0
        );
    }

    // 巣のあるマスと陸全体の比較
    let cells = w.nest.len();
    let cols = w.plants.cols;
    let cs = w.plants.cell_size;
    let center = |c: usize| (((c % cols) as f32 + 0.5) * cs, ((c / cols) as f32 + 0.5) * cs);
    let land: Vec<usize> = (0..cells).filter(|&c| { let (x, y) = center(c); !w.terrain.is_water(x, y) }).collect();
    let nests: Vec<usize> = land.iter().copied().filter(|&c| w.nest[c] > 0.1).collect();
    let mean = |cs: &[usize], f: &dyn Fn(usize) -> f32| cs.iter().map(|&c| f(c)).sum::<f32>() / cs.len().max(1) as f32;
    let wood = |c: usize| w.plants.wood[c];
    let temp = |c: usize| { let (x, y) = center(c); w.terrain.temperature_at(x, y) };
    println!("\n巣のあるマス（出来 0.1 超）{} / 陸 {} マス", nests.len(), land.len());
    println!("  林の度合い: 巣 {:.2} / 陸 {:.2}", mean(&nests, &wood), mean(&land, &wood));
    println!("  今の気温: 巣 {:.1} / 陸 {:.1}", mean(&nests, &temp), mean(&land, &temp));
    println!("  出来の良さの平均（巣のあるマス）: {:.2}", mean(&nests, &|c| w.nest[c]));
    // 持ち主の種類（持ち主が生きている巣だけ）
    let (mut herb, mut carn, mut gone) = (0, 0, 0);
    for &c in &nests {
        match w.index_of(w.nest_owner[c]) {
            Some(i) if w.kind[i] == HERBIVORE => herb += 1,
            Some(_) => carn += 1,
            None => gone += 1,
        }
    }
    println!("  持ち主: 草食 {herb}、肉食 {carn}、死んでいる {gone}");
}

/// 保存ファイルの見出しを読み飛ばし、世界のデータの部分を返す
fn strip_container(data: &[u8]) -> &[u8] {
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = String::from_utf8_lossy(&data[12..12 + len]);
    println!("見出し: {header}");
    &data[12 + len..]
}

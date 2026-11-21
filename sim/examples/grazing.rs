//! 保存ファイル（圧縮なしの .lifesim）を読み込んで世界を進め、草食 1 体を支えている草地の面積を測る。
//!
//! - 陸の面積 ÷ 草食の数（1 体あたりの陸の面積）
//! - 植物の生産量（日射から植物に入ったエネルギー）と、食べられた量（生産量 − 植物量の増え）。
//!   食べられた量 ÷ 草食の数 を、陸 1 マスあたりの生産量で割ると、1 体を支えるのに要る陸のマス数になる
//! - 草が食べ尽くされている度合い（地面の草の層の、上限に対する割合）と、生産量が最大（食べ尽くされた状態）にどれだけ近いか
//! - 草食の混み具合（草食のいるマスの割合、いるマスあたりの数）
//!
//! 使い方:
//!   cargo run --release --features parallel --example grazing --manifest-path sim/Cargo.toml -- world.lifesim [tick 数（既定 2400）]

use sim::world::{CARNIVORE, HERBIVORE, World};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u32 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(2400);
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let (mut w, _) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}  （{ticks} tick 進めて測る）", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));

    let cell = w.plants.cell_size;
    let cap = w.params.plant_capacity;
    let sunlight = w.params.sunlight;
    let total = |w: &World| w.plants.biomass.iter().map(|&b| b as f64).sum::<f64>();
    let (sun0, gain0, bio0) = (w.ledger.sunlight_in, w.ledger.herb_gain, total(&w));
    let (mut herbs, mut land, mut fert, mut fill) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for _ in 0..ticks {
        w.step();
        herbs += w.count_kind(HERBIVORE) as f64;
        // 陸のマス（肥沃度が 0 より大きい）。肥沃度は季節で変わるので毎 tick 数える
        let mut n = 0usize;
        let (mut f_sum, mut fill_sum) = (0.0f64, 0.0f64);
        for (c, &f) in w.plants.fertility.iter().enumerate() {
            if f > 0.0 {
                n += 1;
                f_sum += f as f64;
                // 地面の草の層（草食が主に食べる層）の、今の上限に対する割合
                let ground = w.plants.ground_cap(c) * cap;
                if ground > 0.0 {
                    fill_sum += (w.plants.layer[0][c] / ground).min(1.0) as f64;
                }
            }
        }
        land += n as f64;
        fert += f_sum / n.max(1) as f64;
        fill += fill_sum / n.max(1) as f64;
    }
    let k = ticks as f64;
    let (herbs, land, fert, fill) = (herbs / k, land / k, fert / k, fill / k);
    let produced = (w.ledger.sunlight_in - sun0) / k;
    let eaten = produced - (total(&w) - bio0) / k;
    let gained = (w.ledger.herb_gain - gain0) / k;
    let prod_cell = produced / land;
    let potential = sunlight as f64 * fert;
    let cell_area = (cell * cell) as f64;

    println!("\n陸 {land:.0} マス（1 マス {cell} × {cell} px）、草食の平均 {herbs:.0} 体");
    println!("1 体あたりの陸: {:.2} マス = {:.0} px²（同じ面積の円の半径 {:.1} px）", land / herbs, land / herbs * cell_area, (land / herbs * cell_area / std::f64::consts::PI).sqrt());
    println!(
        "植物の生産: 陸 1 マスあたり {prod_cell:.4} / tick（食べ尽くされた状態の最大 {potential:.4} の {:.0}%）。地面の草は上限の {:.0}%",
        prod_cell / potential * 100.0,
        fill * 100.0
    );
    println!(
        "食べられた量: 全体 {eaten:.1} / tick（生産の {:.0}%）。草食 1 体あたり {:.4} / tick、うち身になった量 {:.4} / tick",
        eaten / produced * 100.0,
        eaten / herbs,
        gained / herbs
    );
    println!(
        "1 体を支えるのに要る陸: {:.2} マス = {:.0} px²（今の生産量で）、最大の生産量なら {:.2} マス",
        eaten / herbs / prod_cell,
        eaten / herbs / prod_cell * cell_area,
        eaten / herbs / potential
    );
    println!("草が食べられてから生え直す時間の目安: 上限 ÷ 日射 = {:.0} tick で、空の状態から上限の 63% まで戻る", cap / sunlight);

    // 混み具合（最後の tick）
    let mut per_cell = vec![0u32; w.plants.biomass.len()];
    for i in 0..w.count() {
        if w.kind[i] == HERBIVORE {
            per_cell[w.plants.cell_index(w.xs[i], w.ys[i])] += 1;
        }
    }
    let occupied = per_cell.iter().filter(|&&c| c > 0).count();
    let n_herb = w.count_kind(HERBIVORE);
    let vision: f32 = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).map(|i| w.traits[i].vision).sum::<f32>() / n_herb.max(1) as f32;
    let disk = std::f64::consts::PI * (vision as f64).powi(2);
    println!(
        "混み具合: 草食のいるマスは陸の {:.0}%、いるマスあたり {:.1} 体。視野（平均 {vision:.0} px）の円に入る草食の数の目安 {:.1} 体",
        occupied as f64 / land * 100.0,
        n_herb as f64 / occupied.max(1) as f64,
        disk / (land / herbs * cell_area)
    );
}

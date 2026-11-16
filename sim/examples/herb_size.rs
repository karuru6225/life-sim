//! 草食が小さくなっていく理由を調べるための、画面なしの実行。草食を大人の大きさで段階に分け、
//! 区間ごとに段階ごとの生まれた数・襲われた数・死因を、1 体・1,000 tick あたりに直して表示する。
//!
//! 使い方（引数は headless と同じ）:
//!   cargo run --release --features parallel --example herb_size --manifest-path sim/Cargo.toml -- 120000 seed=3 every=8000
//!
//! 列の意味（区間内の値。「/体」は区間の始めと終わりの個体数の平均 1 体あたり、1,000 tick あたり）:
//!   大きさ: 大人の大きさの段階     数: 区間の終わりの個体数     割合: 草食全体に占める割合
//!   生: 産んだ子の数（産んだ親の段階で数える）     襲: 肉食に襲われた回数     食: 食べられて死んだ数
//!   逃: 襲われて逃げきれた割合（1 − 食 ÷ 襲）     幼食: 食べられた個体のうち成熟前の割合
//!   餓: 飢えて死んだ数     他: そのほかの死（寿命・病気・争い）     純: 生 − 死の合計（増える勢い）
//!   蓄え: 大人のエネルギー ÷ 繁殖に必要な量 の平均（区間の終わり）
//!   不足: 草を食べようとしたうち、その場所の草が一口に足りなかった割合
//!   取得: 実際に食べた量 ÷ 食べようとした一口の量（大きさに比例する一口のうち、どれだけ取れたか）

use sim::params::Params;
use sim::world::{Census, HERB_SIZE_BINS, HERB_SIZE_EDGES, HERBIVORE, World, death, herb_size_bin};

fn main() {
    let mut ticks: u64 = 120_000;
    let mut every: u64 = 8_000;
    let mut params = Params::default();
    let mut seed = 1;
    for arg in std::env::args().skip(1) {
        if let Some((name, value)) = arg.split_once('=') {
            let value: f32 = value.parse().expect("値が数値ではない");
            match name {
                "seed" => seed = value as u64,
                "every" => every = value as u64,
                _ if !params.set(name, value) => panic!("知らないパラメータ: {name}"),
                _ => {}
            }
        } else {
            ticks = arg.parse().expect("tick 数が数値ではない");
        }
    }

    let mut w = World::new(seed, params);
    let mut last = Census::default();
    let mut last_pop = population(&w);
    for tick in 1..=ticks {
        w.step();
        if tick % every != 0 {
            continue;
        }
        let c = w.census.clone();
        let pop = population(&w);
        let total: usize = pop.iter().sum();
        println!(
            "tick {tick}  草食 {total}  肉食 {}  草食の大人の大きさの平均 {:.2}",
            w.count_kind(sim::world::CARNIVORE),
            mean_adult_size(&w)
        );
        println!(
            "  {:>9} {:>6} {:>5} | {:>6} {:>6} {:>6} {:>5} {:>5} | {:>6} {:>6} | {:>6} | {:>5} | {:>5} {:>5}",
            "大きさ", "数", "割合", "生/体", "襲/体", "食/体", "逃", "幼食", "餓/体", "他/体", "純/体", "蓄え", "不足", "取得"
        );
        let energy = adult_energy(&w);
        for b in 0..HERB_SIZE_BINS {
            let avg = (pop[b] + last_pop[b]) as f64 / 2.0;
            if avg < 1.0 {
                continue;
            }
            let scale = 1000.0 / every as f64 / avg;
            let deaths = |k: usize| (c.herb_size_deaths[b][k] - last.herb_size_deaths[b][k]) as f64;
            let all_deaths: f64 = (1..8).map(deaths).sum();
            let births = (c.herb_size_births[b] - last.herb_size_births[b]) as f64;
            let attacked = (c.herb_size_attacked[b] - last.herb_size_attacked[b]) as f64;
            let eaten = deaths(death::EATEN as usize);
            let starved = deaths(death::STARVED as usize);
            let juvenile_eaten =
                (c.herb_size_juvenile_deaths[b][death::EATEN as usize] - last.herb_size_juvenile_deaths[b][death::EATEN as usize]) as f64;
            let grazes = (c.herb_size_grazes[b] - last.herb_size_grazes[b]) as f64;
            let short = (c.herb_size_graze_short[b] - last.herb_size_graze_short[b]) as f64;
            let bite = c.herb_size_bite[b] - last.herb_size_bite[b];
            let take = c.herb_size_take[b] - last.herb_size_take[b];
            println!(
                "  {:>9} {:>6} {:>5.2} | {:>6.2} {:>6.2} {:>6.2} {:>5.2} {:>5.2} | {:>6.2} {:>6.2} | {:>+6.2} | {:>5.2} | {:>5.2} {:>5.2}",
                label(b),
                pop[b],
                pop[b] as f64 / total.max(1) as f64,
                births * scale,
                attacked * scale,
                eaten * scale,
                if attacked > 0.0 { 1.0 - eaten / attacked } else { f64::NAN },
                if eaten > 0.0 { juvenile_eaten / eaten } else { f64::NAN },
                starved * scale,
                (all_deaths - eaten - starved) * scale,
                (births - all_deaths) * scale,
                energy[b],
                if grazes > 0.0 { short / grazes } else { f64::NAN },
                if bite > 0.0 { take / bite } else { f64::NAN },
            );
        }
        last = c;
        last_pop = pop;
    }
}

/// 段階ごとの草食の数
fn population(w: &World) -> [usize; HERB_SIZE_BINS] {
    let mut pop = [0; HERB_SIZE_BINS];
    for i in (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE) {
        pop[herb_size_bin(w.traits[i].adult_size)] += 1;
    }
    pop
}

/// 段階ごとの、大人のエネルギー ÷ 繁殖に必要な量 の平均
fn adult_energy(w: &World) -> [f64; HERB_SIZE_BINS] {
    let mut sum = [0.0; HERB_SIZE_BINS];
    let mut n = [0usize; HERB_SIZE_BINS];
    for i in (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE && w.growth[i] >= 1.0) {
        let b = herb_size_bin(w.traits[i].adult_size);
        sum[b] += (w.energy[i] / w.repro_threshold(i)) as f64;
        n[b] += 1;
    }
    std::array::from_fn(|b| if n[b] > 0 { sum[b] / n[b] as f64 } else { f64::NAN })
}

fn mean_adult_size(w: &World) -> f64 {
    let sizes: Vec<f64> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).map(|i| w.traits[i].adult_size as f64).collect();
    sizes.iter().sum::<f64>() / sizes.len().max(1) as f64
}

/// 段階の表示（例: 0.65-0.8）
fn label(b: usize) -> String {
    match b {
        0 => format!("<{}", HERB_SIZE_EDGES[0]),
        _ if b == HERB_SIZE_BINS - 1 => format!("{}+", HERB_SIZE_EDGES[b - 1]),
        _ => format!("{}-{}", HERB_SIZE_EDGES[b - 1], HERB_SIZE_EDGES[b]),
    }
}

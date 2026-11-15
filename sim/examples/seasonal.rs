//! 草食の個体数の季節変動の内訳を調べる。区間ごとに、草食の誕生と、死因ごとの死亡（うち幼体）を表示する。
//!
//! 使い方: cargo run --release --example seasonal --manifest-path sim/Cargo.toml -- 12000 seed=1 every=100
//!
//! 列: 季節（−1〜1）、草食の数、幼体の割合、エネルギー ÷ 繁殖に必要な量 の平均、植物量、
//!     区間内の誕生、餓死（うち幼体）、食われ（うち幼体）、寿命、大人の餓死者の平均年齢

use sim::params::Params;
use sim::world::{Census, HERBIVORE, World, death};

fn main() {
    let mut ticks: u64 = 12_000;
    let mut every: u64 = 100;
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
    println!("{:>6} {:>5} {:>6} {:>4} {:>4} {:>6} | {:>5} {:>11} {:>11} {:>4}", "tick", "季節", "草食", "幼", "蓄え", "植物", "生", "餓(幼)", "食われ(幼)", "寿");
    let mut last = Census::default();
    let h = HERBIVORE as usize;
    for tick in 1..=ticks {
        w.step();
        if tick % every != 0 {
            continue;
        }
        let c = w.census.clone();
        let d = |cause: u8| c.deaths[h][cause as usize] - last.deaths[h][cause as usize];
        let j = |cause: u8| c.juvenile_deaths[h][cause as usize] - last.juvenile_deaths[h][cause as usize];
        let herbs: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).collect();
        let n = herbs.len().max(1) as f32;
        let juv = herbs.iter().filter(|&&i| w.growth[i] < 1.0).count() as f32 / n;
        let fed = herbs.iter().map(|&i| w.energy[i] / w.repro_threshold(i)).sum::<f32>() / n;
        println!(
            "{tick:>6} {:>5.2} {:>6} {:>4.2} {:>4.2} {:>6.0} | {:>5} {:>5}({:>4}) {:>5}({:>4}) {:>4}",
            w.season(), herbs.len(), juv, fed, w.plants.total(),
            c.births[h] - last.births[h], d(death::STARVED), j(death::STARVED), d(death::EATEN), j(death::EATEN), d(death::OLD_AGE)
        );
        last = c;
    }
}

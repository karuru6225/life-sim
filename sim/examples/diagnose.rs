//! 肉食の増減の原因を調べるための、画面なしの実行。区間ごとに肉食の生死と狩りの内訳を表示する。
//!
//! 使い方（引数は headless と同じ）:
//!   cargo run --release --example diagnose --manifest-path sim/Cargo.toml -- 80000 seed=3 every=2000
//!
//! 列の意味（区間内の値。「/体」は区間の終わりの肉食 1 体あたり）:
//!   生/餓/寿/傷: 肉食の誕生・餓死・寿命死・反撃による死
//!   食われ: 草食が食べられた数
//!   襲撃/体: 肉食 1 体が区間内に襲いかかった回数
//!   成功: 襲撃のうち仕留めた割合     見込: 襲撃時の成功の見込みの平均
//!   比: 襲撃時の 獲物の大きさ ÷ 自分の大きさ（協力込み）の平均
//!   守/協/干: 襲撃時の 獲物の近くの草食の数・近くの同系統の肉食の数・近くの系統の違う肉食の数 の平均
//!   分: 仕留めた獲物を分け合った人数の平均
//!   見えず/追跡/求愛: 区間の終わりに、獲物が見えていない・獲物が見えている・繁殖相手へ向かっている肉食の割合
//!   蓄え: 肉食のエネルギー ÷ 繁殖に必要な量 の平均     弱: 体の調子が 1 未満（飢えや老いで遅い）の割合
//!   幼: 成熟していない肉食の割合
//!   獲得/殺: 仕留めた獲物 1 体から肉食全体が得たエネルギー     草蓄: 草食のエネルギーの平均
//!   繁可: 繁殖できる状態（成熟していてエネルギーが足りている）の肉食の割合。高いのに生まれる数が少なければ、相手が見つからない
//!   大きさ/速度/視野: 肉食の大人の大きさ・最高速度・視野の平均
//!   系統: 系統色を 36 段階に分けたとき、3 体以上いる段階の数（肉食/草食）

use sim::params::Params;
use sim::world::{CARNIVORE, Census, HERBIVORE, World, death, state};

fn main() {
    let mut ticks: u64 = 80_000;
    let mut every: u64 = 2_000;
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
    println!(
        "{:>6} {:>6} {:>5} {:>6} | {:>4} {:>4} {:>4} {:>3} {:>5} | {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>3} | {:>4} {:>4} {:>4} | {:>4} {:>4} {:>4} | {:>5} {:>4} {:>4} | {:>4} {:>4} {:>4} | {:>5}",
        "tick", "草食", "肉食", "植物", "生", "餓", "寿", "傷", "食われ", "襲撃/体", "成功", "見込", "比", "守", "協", "干", "分",
        "見えず", "追跡", "求愛", "蓄え", "弱", "幼", "獲得/殺", "草蓄", "繁可", "大きさ", "速度", "視野", "系統"
    );
    let mut last = Census::default();
    let mut last_gain = 0.0f64;
    for tick in 1..=ticks {
        w.step();
        if tick % every != 0 {
            continue;
        }
        let c = w.census.clone();
        let carn = CARNIVORE as usize;
        let d = |k: usize, cause: u8| c.deaths[k][cause as usize] - last.deaths[k][cause as usize];
        let hunts = (c.hunts - last.hunts) as f64;
        let kills = (c.kills - last.kills) as f64;
        let per_hunt = |now: f64, before: f64| if hunts > 0.0 { (now - before) / hunts } else { f64::NAN };

        let carns: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE).collect();
        let n = carns.len().max(1) as f64;
        let share = |f: &dyn Fn(usize) -> bool| carns.iter().filter(|&&i| f(i)).count() as f64 / n;
        let mean = |f: &dyn Fn(usize) -> f32| carns.iter().map(|&i| f(i) as f64).sum::<f64>() / n;
        let lineages = |kind: u8| {
            let mut bins = [0u32; 36];
            for i in (0..w.count()).filter(|&i| w.kind[i] == kind) {
                bins[((w.traits[i].hue * 36.0) as usize).min(35)] += 1;
            }
            bins.iter().filter(|&&b| b >= 3).count()
        };

        println!(
            "{tick:>6} {:>6} {:>5} {:>6.0} | {:>4} {:>4} {:>4} {:>3} {:>5} | {:>5.2} {:>4.2} {:>4.2} {:>4.2} {:>4.1} {:>4.1} {:>4.2} {:>3.1} | {:>4.2} {:>4.2} {:>4.2} | {:>4.2} {:>4.2} {:>4.2} | {:>5.2} {:>4.2} {:>4.2} | {:>4.2} {:>4.2} {:>4.1} | {:>2}/{:<2}",
            w.count_kind(HERBIVORE),
            carns.len(),
            w.plants.total(),
            c.births[carn] - last.births[carn],
            d(carn, death::STARVED),
            d(carn, death::OLD_AGE),
            d(carn, death::INJURED),
            d(HERBIVORE as usize, death::EATEN),
            hunts / n,
            if hunts > 0.0 { kills / hunts } else { f64::NAN },
            per_hunt(c.hunt_chance, last.hunt_chance),
            per_hunt(c.hunt_ratio, last.hunt_ratio),
            per_hunt(c.hunt_guards, last.hunt_guards),
            per_hunt(c.hunt_allies, last.hunt_allies),
            per_hunt(c.hunt_rivals, last.hunt_rivals),
            if kills > 0.0 { (c.kill_eaters - last.kill_eaters) / kills } else { f64::NAN },
            share(&|i| w.state[i] == state::WANDER),
            share(&|i| w.state[i] == state::CHASE || w.state[i] == state::EAT || w.state[i] == state::MISSED),
            share(&|i| w.state[i] == state::COURT),
            mean(&|i| w.energy[i] / w.repro_threshold(i)),
            share(&|i| w.vigor(i) < 1.0),
            share(&|i| w.growth[i] < 1.0),
            if kills > 0.0 { (w.ledger.carn_gain - last_gain) / kills } else { f64::NAN },
            {
                let h: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).map(|i| w.energy[i]).collect();
                h.iter().sum::<f32>() / h.len().max(1) as f32
            },
            share(&|i| w.growth[i] >= 1.0 && w.energy[i] >= w.repro_threshold(i)),
            mean(&|i| w.traits[i].adult_size),
            mean(&|i| w.traits[i].max_speed),
            mean(&|i| w.traits[i].vision),
            lineages(CARNIVORE),
            lineages(HERBIVORE),
        );
        last = c;
        last_gain = w.ledger.carn_gain;
        if carns.is_empty() {
            println!("肉食が絶滅");
            break;
        }
    }
}

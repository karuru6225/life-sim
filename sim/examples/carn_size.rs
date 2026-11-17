//! 肉食が大きくなっていく理由を調べるための、画面なしの実行。肉食を大人の大きさで段階に分け、
//! 区間ごとに段階ごとの生まれた数・狩り・死因を、1 体・1,000 tick あたりに直して表示する。
//!
//! 保存ファイルの続きを回すときは load=ファイル（画面なしの実行器で書き出した圧縮なしの .lifesim）を付ける。
//!
//! 使い方（引数は headless と同じ）:
//!   cargo run --release --features parallel --example carn_size --manifest-path sim/Cargo.toml -- 120000 seed=3 every=8000
//!
//! 列の意味（区間内の値。「/体」は区間の始めと終わりの個体数の平均 1 体あたり、1,000 tick あたり）:
//!   大きさ: 大人の大きさの段階     数: 区間の終わりの個体数     割合: 肉食全体に占める割合
//!   生: 産んだ子の数（産んだ親の段階で数える）     襲: 襲った回数     成功: 仕留めた割合
//!   比: 襲った獲物の大きさ ÷ 自分の大きさ の平均     得: 狩りで得たエネルギー（仲間との分け前を含む）
//!   費: 襲う費用     餓: 飢えて死んだ数     老: 寿命     傷: 狩りの反撃のけが     病: 病気
//!   争: 争いで死んだ数（よそ者・仲間とも）     純: 生 − 死の合計（増える勢い）
//!   蓄え: 大人のエネルギー ÷ 繁殖に必要な量 の平均（区間の終わり）
//!   得/大: 得 ÷ 段階の大人の大きさの平均（体重あたりの収入）

use sim::params::Params;
use sim::world::{CARN_SIZE_BINS, CARN_SIZE_EDGES, CARNIVORE, Census, HERBIVORE, World, carn_size_bin, death};

fn main() {
    let mut ticks: u64 = 120_000;
    let mut every: u64 = 8_000;
    let mut params = Params::default();
    let mut seed = 1;
    let mut load: Option<String> = None;
    let mut overrides: Vec<(String, f32)> = Vec::new();
    for arg in std::env::args().skip(1) {
        if let Some((name, value)) = arg.split_once('=') {
            if name == "load" {
                load = Some(value.to_string());
                continue;
            }
            let value: f32 = value.parse().expect("値が数値ではない");
            overrides.push((name.to_string(), value));
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

    // load=保存ファイル（圧縮なしの .lifesim）を付けると、その世界の続きを回す。パラメータの指定は読み込んだ世界に上書きする
    let mut w = match &load {
        Some(path) => {
            let data = std::fs::read(path).expect("保存ファイルを読めない");
            let (mut w, _) = sim::world::load_file(&data).unwrap_or_else(|e| panic!("読み込めない: {e}"));
            for (name, value) in &overrides {
                if name != "seed" && name != "every" {
                    w.params.set(name, *value);
                }
            }
            println!("{path} の tick {} から続ける", w.tick);
            w
        }
        None => World::new(seed, params),
    };
    // 累計の基準（読み込んだ世界は累計が 0 から始まらない）
    let mut last = w.census.clone();
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
            "tick {tick}  肉食 {total}  草食 {}  肉食の大人の大きさの平均 {:.2}  草食の大人の大きさの平均 {:.2}",
            w.count_kind(HERBIVORE),
            mean_adult_size(&w, CARNIVORE),
            mean_adult_size(&w, HERBIVORE)
        );
        println!(
            "  {:>9} {:>5} {:>5} | {:>6} {:>6} {:>5} {:>5} | {:>6} {:>6} {:>6} | {:>6} {:>6} {:>6} {:>6} {:>6} | {:>6} | {:>5}",
            "大きさ", "数", "割合", "生/体", "襲/体", "成功", "比", "得/体", "得/大", "費/体", "餓/体", "老/体", "傷/体", "病/体", "争/体", "純/体", "蓄え"
        );
        let (energy, sizes) = adult_stats(&w);
        for b in 0..CARN_SIZE_BINS {
            let avg = (pop[b] + last_pop[b]) as f64 / 2.0;
            if avg < 1.0 {
                continue;
            }
            let scale = 1000.0 / every as f64 / avg;
            let deaths = |k: usize| (c.carn_size_deaths[b][k] - last.carn_size_deaths[b][k]) as f64;
            let all_deaths: f64 = (1..8).map(deaths).sum();
            let starved = deaths(death::STARVED as usize);
            let births = (c.carn_size_births[b] - last.carn_size_births[b]) as f64;
            let hunts = (c.carn_size_hunts[b] - last.carn_size_hunts[b]) as f64;
            let kills = (c.carn_size_kills[b] - last.carn_size_kills[b]) as f64;
            let ratio = c.carn_size_ratio[b] - last.carn_size_ratio[b];
            let gain = c.carn_size_gain[b] - last.carn_size_gain[b];
            let cost = c.carn_size_attack_cost[b] - last.carn_size_attack_cost[b];
            println!(
                "  {:>9} {:>5} {:>5.2} | {:>6.2} {:>6.2} {:>5.2} {:>5.2} | {:>6.2} {:>6.2} {:>6.2} | {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.2} | {:>+6.2} | {:>5.2}",
                label(b),
                pop[b],
                pop[b] as f64 / total.max(1) as f64,
                births * scale,
                hunts * scale,
                if hunts > 0.0 { kills / hunts } else { f64::NAN },
                if hunts > 0.0 { ratio / hunts } else { f64::NAN },
                gain * scale,
                gain * scale / sizes[b],
                cost * scale,
                starved * scale,
                deaths(death::OLD_AGE as usize) * scale,
                deaths(death::INJURED as usize) * scale,
                deaths(death::SICK as usize) * scale,
                (deaths(death::FOUGHT as usize) + deaths(death::KIN_FOUGHT as usize)) * scale,
                (births - all_deaths) * scale,
                energy[b],
            );
        }
        last = c;
        last_pop = pop;
    }
}

/// 段階ごとの肉食の数
fn population(w: &World) -> [usize; CARN_SIZE_BINS] {
    let mut pop = [0; CARN_SIZE_BINS];
    for i in (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE) {
        pop[carn_size_bin(w.traits[i].adult_size)] += 1;
    }
    pop
}

/// 段階ごとの (大人のエネルギー ÷ 繁殖に必要な量 の平均, 大人の大きさの平均)
fn adult_stats(w: &World) -> ([f64; CARN_SIZE_BINS], [f64; CARN_SIZE_BINS]) {
    let mut energy = [0.0; CARN_SIZE_BINS];
    let mut size = [0.0; CARN_SIZE_BINS];
    let mut n = [0usize; CARN_SIZE_BINS];
    let mut all = [0usize; CARN_SIZE_BINS];
    for i in (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE) {
        let b = carn_size_bin(w.traits[i].adult_size);
        size[b] += w.traits[i].adult_size as f64;
        all[b] += 1;
        if w.growth[i] >= 1.0 {
            energy[b] += (w.energy[i] / w.repro_threshold(i)) as f64;
            n[b] += 1;
        }
    }
    (
        std::array::from_fn(|b| if n[b] > 0 { energy[b] / n[b] as f64 } else { f64::NAN }),
        std::array::from_fn(|b| if all[b] > 0 { size[b] / all[b] as f64 } else { f64::NAN }),
    )
}

fn mean_adult_size(w: &World, kind: u8) -> f64 {
    let sizes: Vec<f64> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.traits[i].adult_size as f64).collect();
    sizes.iter().sum::<f64>() / sizes.len().max(1) as f64
}

/// 段階の表示（例: 1.1-1.3）
fn label(b: usize) -> String {
    match b {
        0 => format!("<{}", CARN_SIZE_EDGES[0]),
        _ if b == CARN_SIZE_BINS - 1 => format!("{}+", CARN_SIZE_EDGES[b - 1]),
        _ => format!("{}-{}", CARN_SIZE_EDGES[b - 1], CARN_SIZE_EDGES[b]),
    }
}

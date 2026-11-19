//! 肉食 1 体ずつの一生を追い、繁殖のどの段で止まっているかを測る。画面なしで世界を進める。
//!
//! 使い方（引数は diagnose と同じ）:
//!   cargo run --release --features parallel --example carn_life --manifest-path sim/Cargo.toml -- 16000 seed=1 every=2000
//!
//! 区間ごとの表（区間内の値）:
//!   成熟前/休み/蓄え不足/準備済: 肉食の個体 × tick のうち、成熟していない・繁殖の休み中・蓄えが繁殖に要る量に足りない・繁殖できる、の割合
//!   準備済の内訳: 繁殖を望まなかった・望んだが相手がいなかった・交配を試みた の割合
//!   待ち: 交配 1 回あたり、繁殖できる状態で相手を待った tick 数（相手がいなかった tick ÷ 試みた tick）
//!   蓄え: 大人で休み明けの肉食の、エネルギー ÷ 繁殖に要る量 の平均
//!
//! 最後の表（生まれた区間ごとの一生。死んだ個体だけ。初期 = 世界を作ったときに置いた個体）:
//!   数: 死んだ個体の数   成熟: 成熟まで生きた割合   繁殖: 一度でも産んだ割合
//!   子/体: 1 体が一生に産んだ子の数の平均（産んだ側で数える。1 を下回ると、数が保てない）
//!   回/体: 一生に産んだ回数の平均   初産: 生まれてから初めて産むまでの tick（産んだ個体の平均）
//!   寿命比: 死んだときの年齢 ÷ その個体の寿命   殺/体: 一生に仕留めた獲物の数
//!   死因: 餓死・老衰・反撃でけが・敵（系統の違う肉食）との争い・仲間との争い の割合

use sim::params::Params;
use sim::world::{CARNIVORE, World, death};
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct Rec {
    /// 生まれた tick（初期個体は 0）
    born: u64,
    initial: bool,
    matured: Option<u64>,
    first_birth: Option<u64>,
    /// 産んだ回数と子の数
    litters: u32,
    children: u32,
    age: u32,
    lifespan: f32,
    kills: u32,
}

/// 生まれた区間ごとの集計
#[derive(Default, Clone, Copy)]
struct Cohort {
    n: u32,
    matured: u32,
    bred: u32,
    children: u64,
    litters: u64,
    first_birth_age: u64,
    age_ratio: f64,
    kills: u64,
    causes: [u32; 8],
}

fn main() {
    let mut ticks: u64 = 16_000;
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
    let mut recs: HashMap<u32, Rec> = HashMap::new();
    for i in (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE) {
        recs.insert(
            w.id[i],
            Rec {
                born: 0,
                initial: true,
                matured: Some(0),
                first_birth: None,
                litters: 0,
                children: 0,
                age: w.age[i],
                lifespan: w.lifespan(i),
                kills: 0,
            },
        );
    }
    let n_cohorts = (ticks / every) as usize + 2; // 0 = 初期、k = 区間 k で生まれた
    let mut cohorts = vec![Cohort::default(); n_cohorts];
    // 区間内の 個体 × tick の内訳 [成熟前, 休み, 蓄え不足, 準備済]
    let mut blocks = [0u64; 4];
    let (mut fill_sum, mut fill_n) = (0.0f64, 0u64);
    let mut last_ready = w.census.ready[CARNIVORE as usize];

    println!(
        "{:>6} {:>5} | {:>6} {:>5} {:>8} {:>6} | {:>8} {:>8} {:>8} {:>5} | {:>5} | {:>5} {:>5}",
        "tick", "肉食", "成熟前", "休み", "蓄え不足", "準備済", "望まない", "相手なし", "試みた", "待ち", "蓄え", "生", "死"
    );
    let (mut born_window, mut died_window) = (0u32, 0u32);
    for tick in 1..=ticks {
        w.step();
        // 死んだ個体を締める
        for &(id, cause) in &w.step_deaths {
            if let Some(r) = recs.remove(&id) {
                died_window += 1;
                let k = if r.initial { 0 } else { (r.born / every) as usize + 1 };
                let c = &mut cohorts[k.min(n_cohorts - 1)];
                c.n += 1;
                c.matured += r.matured.is_some() as u32;
                c.bred += (r.litters > 0) as u32;
                c.children += r.children as u64;
                c.litters += r.litters as u64;
                if let Some(t) = r.first_birth {
                    c.first_birth_age += t - r.born;
                }
                c.age_ratio += r.age as f64 / r.lifespan.max(1.0) as f64;
                c.kills += r.kills as u64;
                c.causes[cause as usize] += 1;
            }
        }
        // 生きている肉食を更新する
        for i in (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE) {
            let r = recs.entry(w.id[i]).or_insert_with(|| {
                born_window += 1;
                Rec {
                    born: tick,
                    initial: false,
                    matured: None,
                    first_birth: None,
                    litters: 0,
                    children: 0,
                    age: 0,
                    lifespan: w.lifespan(i),
                    kills: 0,
                }
            });
            if w.children[i] > r.children {
                r.litters += 1;
                r.children = w.children[i];
                r.first_birth.get_or_insert(tick);
            }
            if r.matured.is_none() && w.growth[i] >= 1.0 {
                r.matured = Some(tick);
            }
            r.age = w.age[i];
            r.kills = w.life[i].kills;
            let k = if w.growth[i] < 1.0 {
                0
            } else if w.rest[i] > 0 {
                1
            } else if w.energy[i] < w.repro_threshold(i) {
                2
            } else {
                3
            };
            blocks[k] += 1;
            if k >= 2 {
                fill_sum += (w.energy[i] / w.repro_threshold(i)) as f64;
                fill_n += 1;
            }
        }

        if tick % every == 0 {
            let total = blocks.iter().sum::<u64>().max(1) as f64;
            let ready = w.census.ready[CARNIVORE as usize];
            let d: Vec<u64> = (0..3).map(|k| ready[k] - last_ready[k]).collect();
            let rt = d.iter().sum::<u64>().max(1) as f64;
            println!(
                "{tick:>6} {:>5} | {:>6.2} {:>5.2} {:>8.2} {:>6.2} | {:>8.2} {:>8.2} {:>8.2} {:>5.0} | {:>5.2} | {:>5} {:>5}",
                w.count_kind(CARNIVORE),
                blocks[0] as f64 / total,
                blocks[1] as f64 / total,
                blocks[2] as f64 / total,
                blocks[3] as f64 / total,
                d[0] as f64 / rt,
                d[1] as f64 / rt,
                d[2] as f64 / rt,
                d[1] as f64 / d[2].max(1) as f64,
                fill_sum / fill_n.max(1) as f64,
                born_window,
                died_window,
            );
            blocks = [0; 4];
            (fill_sum, fill_n) = (0.0, 0);
            last_ready = ready;
            (born_window, died_window) = (0, 0);
            if w.count_kind(CARNIVORE) == 0 {
                println!("肉食が絶滅");
                break;
            }
        }
    }

    println!(
        "\n{:>13} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6} | 死因 {:>5} {:>5} {:>5} {:>5} {:>5}",
        "生まれ", "数", "成熟", "繁殖", "子/体", "回/体", "初産", "寿命比", "殺/体", "餓死", "老衰", "けが", "敵", "仲間"
    );
    for (k, c) in cohorts.iter().enumerate() {
        if c.n == 0 {
            continue;
        }
        let label = if k == 0 { "初期".to_string() } else { format!("{}〜{}", (k as u64 - 1) * every, k as u64 * every) };
        let n = c.n as f64;
        let share = |v: u32| v as f64 / n;
        println!(
            "{label:>13} {:>5} {:>5.2} {:>5.2} {:>6.2} {:>6.2} {:>6.0} {:>6.2} {:>6.1} |      {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2}",
            c.n,
            share(c.matured),
            share(c.bred),
            c.children as f64 / n,
            c.litters as f64 / n,
            c.first_birth_age as f64 / c.bred.max(1) as f64,
            c.age_ratio / n,
            c.kills as f64 / n,
            share(c.causes[death::STARVED as usize]),
            share(c.causes[death::OLD_AGE as usize]),
            share(c.causes[death::INJURED as usize]),
            share(c.causes[death::FOUGHT as usize]),
            share(c.causes[death::KIN_FOUGHT as usize]),
        );
    }
    let alive = recs.len();
    let alive_children: u64 = recs.values().map(|r| r.children as u64).sum();
    println!("\n生きている肉食 {alive} 体（これまでに産んだ子 {alive_children} 体）。上の表には入っていない");
}

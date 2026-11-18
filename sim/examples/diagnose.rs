//! 肉食の増減の原因を調べるための、画面なしの実行。区間ごとに肉食の生死と狩りの内訳を表示する。
//!
//! 使い方（引数は headless と同じ）:
//!   cargo run --release --features parallel --example diagnose --manifest-path sim/Cargo.toml -- 80000 seed=3 every=2000
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
//!   争: 同じ種類の敵への攻撃の回数（肉食/草食）     争死: 敵との争いで死んだ数（肉食/草食）
//!   仲争: 仲間（同じ系統）への攻撃の回数（肉食/草食）   仲死: 仲間との争いで死んだ数（肉食/草食）
//!   幼餓: 餓死した肉食のうち成熟前だった数
//!   見獲: 区間の終わりに獲物が見えている肉食の割合     視内: 肉食の視野の円の中にいる草食の数の平均（視野角は無視）
//!   走: 肉食の最後の tick の速さ ÷ 最高速度 の平均     体力/疲: 肉食の体力の平均と、0.2 を切った割合
//!   得/体t: 区間内に肉食 1 体が 1 tick に獲物から得たエネルギー     増/体t: 肉食 1 体のエネルギーの 1 tick あたりの増減
//!   飢近: エネルギーが繁殖に必要な量の 1 割を切っている肉食の割合
//!   費用（肉食 1 体の 1 tick あたり ×1000）: 基=基礎代謝 維=速さの維持 温=体温 目=目 移=移動 他=シグナル・脳・記憶 襲=襲う費用
//!   得/体t（×1000 にすると比べやすい）と、これらの合計との差が、狩りの収支

use sim::params::Params;
use sim::genome::IN_PREY_NEAR;
use sim::world::{CARNIVORE, Census, HERBIVORE, World, death, state, torus_delta};

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
        "{:>6} {:>6} {:>5} {:>6} | {:>4} {:>4} {:>4} {:>3} {:>5} | {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>3} | {:>4} {:>4} {:>4} | {:>4} {:>4} {:>4} | {:>5} {:>4} {:>4} | {:>4} {:>4} {:>4} | {:>5} | {:>11} | {:>9} | {:>11} | {:>9} | {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>6} {:>6} {:>4} | {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4}",
        "tick", "草食", "肉食", "植物", "生", "餓", "寿", "傷", "食われ", "襲撃/体", "成功", "見込", "比", "守", "協", "干", "分",
        "見えず", "追跡", "求愛", "蓄え", "弱", "幼", "獲得/殺", "草蓄", "繁可", "大きさ", "速度", "視野", "系統", "争", "争死", "仲争", "仲死", "幼餓", "見獲", "視内", "走", "体力", "疲", "得/体t", "増/体t", "飢近", "基", "維", "温", "目", "移", "他", "襲"
    );
    let mut last = Census::default();
    let mut last_gain = 0.0f64;
    let carn_energy = |w: &World| (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE).map(|i| w.energy[i] as f64).sum::<f64>();
    let mut last_energy = carn_energy(&w);
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

        let inputs = w.brain_inputs();
        let carns: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE).collect();
        let herbs: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).collect();
        // 視野の円の中の草食の数（肉食 1 体あたり）
        let in_view = carns.iter().map(|&i| {
            let r2 = w.traits[i].vision * w.traits[i].vision;
            herbs.iter().filter(|&&j| {
                let dx = torus_delta(w.xs[j] - w.xs[i], w.width);
                let dy = torus_delta(w.ys[j] - w.ys[i], w.height);
                dx * dx + dy * dy <= r2
            }).count() as f64
        }).sum::<f64>() / carns.len().max(1) as f64;
        let energy_now = carn_energy(&w);
        // 費用は区間の終わりの肉食の数で割る（区間の途中で減っていると、やや大きめに出る）
        let per_carn_tick = 1000.0 / (carns.len().max(1) as f64 * every as f64);
        let cost = |k: usize| (w.census.costs[CARNIVORE as usize][k] - last.costs[CARNIVORE as usize][k]) * per_carn_tick;
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
            "{tick:>6} {:>6} {:>5} {:>6.0} | {:>4} {:>4} {:>4} {:>3} {:>5} | {:>5.2} {:>4.2} {:>4.2} {:>4.2} {:>4.1} {:>4.1} {:>4.2} {:>3.1} | {:>4.2} {:>4.2} {:>4.2} | {:>4.2} {:>4.2} {:>4.2} | {:>5.2} {:>4.2} {:>4.2} | {:>4.2} {:>4.2} {:>4.1} | {:>2}/{:<2} | {:>5}/{:<5} | {:>4}/{:<4} | {:>5}/{:<5} | {:>4}/{:<4} | {:>4} {:>4.2} {:>4.1} {:>4.2} {:>4.2} {:>4.2} {:>6.4} {:>6.4} {:>4.2} | {:>4.1} {:>4.1} {:>4.1} {:>4.1} {:>4.1} {:>4.1} {:>4.1}",
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
            share(&|i| w.can_breed(i)),
            mean(&|i| w.traits[i].adult_size),
            mean(&|i| w.traits[i].max_speed),
            mean(&|i| w.traits[i].vision),
            lineages(CARNIVORE),
            lineages(HERBIVORE),
            c.fights[carn] - last.fights[carn],
            c.fights[HERBIVORE as usize] - last.fights[HERBIVORE as usize],
            d(carn, death::FOUGHT),
            d(HERBIVORE as usize, death::FOUGHT),
            c.kin_fights[carn] - last.kin_fights[carn],
            c.kin_fights[HERBIVORE as usize] - last.kin_fights[HERBIVORE as usize],
            d(carn, death::KIN_FOUGHT),
            d(HERBIVORE as usize, death::KIN_FOUGHT),
            c.juvenile_deaths[carn][death::STARVED as usize] - last.juvenile_deaths[carn][death::STARVED as usize],
            share(&|i| inputs[i][IN_PREY_NEAR] > 0.0),
            in_view,
            mean(&|i| (w.vel[i].0.hypot(w.vel[i].1)) / w.traits[i].max_speed.max(1e-3)),
            mean(&|i| w.stamina[i]),
            share(&|i| w.stamina[i] < 0.2),
            (w.ledger.carn_gain - last_gain) / (n * every as f64),
            (energy_now - last_energy) / (n * every as f64),
            share(&|i| w.energy[i] < 0.1 * w.repro_threshold(i)),
            cost(0), cost(1), cost(2), cost(4), cost(7), cost(3) + cost(5) + cost(6),
            (c.carn_size_attack_cost.iter().sum::<f64>() - last.carn_size_attack_cost.iter().sum::<f64>()) * per_carn_tick,
        );
        last_energy = energy_now;
        last = c;
        last_gain = w.ledger.carn_gain;
        if carns.is_empty() {
            println!("肉食が絶滅");
            break;
        }
    }
}

//! 保存ファイル（圧縮なしの .lifesim）を読み込み、肉食の一団を 1 体ずつ追って、どこで子を残せずに終わるかを数える。
//!
//! 追う一団は、x y を指定すればそこに出した肉食（画面の「肉食を出す」と同じ操作、30 体）、指定しなければ今いる肉食全員。
//! 一団から生まれた子孫も追う。1 体ごとに次を記録する:
//! - 死んだ理由と年齢
//! - 蓄えが繁殖に足りていた（エネルギー ≥ 繁殖に必要な量）時間の割合
//! - 繁殖できた（成熟・蓄え十分・休み明け・妊娠していない）時間と、そのうち繁殖相手を探せる距離に成熟した仲間がいた割合
//! - 交配を試みた回数と、相性で失敗した回数。産んだ子の数
//! - 襲った回数・仕留めた数、最も近い仲間までの距離
//!
//! 使い方:
//!   cargo run --release --example carn_fate --manifest-path sim/Cargo.toml -- world.lifesim [tick 数（既定 6000）] [x y]

use sim::world::{CARNIVORE, World, torus_delta};
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone)]
struct Fate {
    generation: u8, // 0 = 一団、1 = その子孫
    adult_ticks: u32,
    fed_ticks: u32,
    breed_ticks: u32,
    breed_with_mate: u32,
    near_sum: f32,
    ticks: u32,
    kids: u32,
    matings: u32,
    failed: u32,
    hunts: u32,
    kills: u32,
    death: Option<(u8, u32)>, // (死因, 年齢)
}

const CAUSES: [&str; 8] = ["?", "餓死", "老衰", "食べられた", "狩りの反撃", "病気", "敵との争い", "仲間との争い"];

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u32 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(6000);
    let spot: Option<(f32, f32)> = match (args.next(), args.next()) {
        (Some(x), Some(y)) => Some((x.parse().unwrap(), y.parse().unwrap())),
        _ => None,
    };
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let (mut w, _) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    let range = w.params.carn_mate_range;
    println!("tick {}  肉食 {}  繁殖相手を探せる距離 {range} px", w.tick, w.count_kind(CARNIVORE));

    let carns = |w: &World| (0..w.count()).filter(|&i| w.kind[i] == CARNIVORE).collect::<Vec<_>>();
    let mut fates: HashMap<u32, Fate> = HashMap::new();
    if let Some((x, y)) = spot {
        let before = w.count();
        let hue = w.spawn_lineage_at(CARNIVORE, x, y, 30, 24.0, 1).expect("出せない（水の上か、肉食がいない）");
        println!("({x}, {y}) に系統色 {hue:.2} の肉食を 30 体出した");
        for i in before..w.count() {
            fates.insert(w.id[i], Fate::default());
        }
    } else {
        for i in carns(&w) {
            fates.insert(w.id[i], Fate::default());
        }
    }
    let mut known: HashSet<u32> = carns(&w).iter().map(|&i| w.id[i]).collect();

    for t in 0..ticks {
        let c = carns(&w);
        // 新しく生まれた肉食のうち、親が追っている個体なら子孫として追う
        for &i in &c {
            let id = w.id[i];
            if known.insert(id) {
                let parents = w.parents[i];
                if parents.iter().any(|p| fates.contains_key(p)) {
                    for p in parents {
                        if let Some(f) = fates.get_mut(&p) {
                            f.kids += 1;
                        }
                    }
                    fates.insert(id, Fate { generation: 1, ..Default::default() });
                }
            }
        }
        for &i in &c {
            let Some(f) = fates.get_mut(&w.id[i]) else { continue };
            let (x, y) = (w.xs[i], w.ys[i]);
            let mut nearest = f32::MAX;
            let mut mate_near = false;
            for &j in &c {
                if j == i {
                    continue;
                }
                let (dx, dy) = (torus_delta(w.xs[j] - x, w.width), torus_delta(w.ys[j] - y, w.height));
                let d = (dx * dx + dy * dy).sqrt();
                nearest = nearest.min(d);
                mate_near |= d <= range && w.growth[j] >= 1.0;
            }
            f.ticks += 1;
            f.near_sum += nearest.min(2000.0);
            if w.growth[i] >= 1.0 {
                f.adult_ticks += 1;
                f.fed_ticks += (w.energy[i] >= w.repro_threshold(i)) as u32;
            }
            if w.can_breed(i) {
                f.breed_ticks += 1;
                f.breed_with_mate += mate_near as u32;
            }
            let l = &w.life[i];
            (f.matings, f.failed, f.hunts, f.kills) = (l.matings, l.failed, l.hunts, l.kills);
        }
        let alive_before: Vec<(u32, u32)> = c.iter().map(|&i| (w.id[i], w.age[i])).collect();
        let deaths0 = w.census.deaths[CARNIVORE as usize];
        w.step();
        // この tick に死んだ個体の死因。同じ tick に違う死因で複数死んだら決められない（?）
        let now: HashSet<u32> = carns(&w).iter().map(|&i| w.id[i]).collect();
        let dead: Vec<(u32, u32)> = alive_before.into_iter().filter(|(id, _)| !now.contains(id)).collect();
        let d = w.census.deaths[CARNIVORE as usize];
        let causes: Vec<u8> = (1..8).filter(|&k| d[k] > deaths0[k]).map(|k| k as u8).collect();
        let cause = if causes.len() == 1 { causes[0] } else { 0 };
        for (id, age) in dead {
            if let Some(f) = fates.get_mut(&id) {
                f.death = Some((cause, age));
            }
        }
        if (t + 1) % 1000 == 0 {
            let alive = |g: u8| fates.values().filter(|f| f.generation == g && f.death.is_none()).count();
            println!("  +{:5} tick: 肉食 {}、追っている一団で生きている {}、その子孫で生きている {}", t + 1, now.len(), alive(0), alive(1));
        }
    }

    println!("\n{} tick 進めた（tick {} まで）", ticks, w.tick);
    for (g, name) in [(0u8, "一団"), (1, "その子孫")] {
        let v: Vec<&Fate> = fates.values().filter(|f| f.generation == g).collect();
        if v.is_empty() {
            continue;
        }
        let n = v.len() as f32;
        let sum = |k: &dyn Fn(&Fate) -> f32| v.iter().map(|f| k(f)).sum::<f32>();
        println!("\n{name} {} 体", v.len());
        let mut causes = [0u32; 8];
        let mut ages = [0f32; 8];
        for f in &v {
            if let Some((c, a)) = f.death {
                causes[c as usize] += 1;
                ages[c as usize] += a as f32;
            }
        }
        let dead: u32 = causes.iter().sum();
        let parts: Vec<String> = (0..8)
            .filter(|&k| causes[k] > 0)
            .map(|k| format!("{} {}（平均 {:.0} 歳）", CAUSES[k], causes[k], ages[k] / causes[k] as f32))
            .collect();
        println!("  死んだ {dead} 体: {}。生きている {}", parts.join("、"), v.len() as u32 - dead);
        let adult = sum(&|f| f.adult_ticks as f32);
        let breed = sum(&|f| f.breed_ticks as f32);
        println!(
            "  大人でいた時間のうち、蓄えが繁殖に足りていた {:.0}%、繁殖できた {:.0}%（そのうち {range} px 以内に成熟した仲間がいた {:.0}%）",
            sum(&|f| f.fed_ticks as f32) / adult.max(1.0) * 100.0,
            breed / adult.max(1.0) * 100.0,
            sum(&|f| f.breed_with_mate as f32) / breed.max(1.0) * 100.0
        );
        println!(
            "  一度でも繁殖できる状態になった {:.0}%。交配を試みた {:.0} 回（相性で失敗 {:.0}）、産んだ子 {:.0} 体（1 体あたり {:.2}）",
            v.iter().filter(|f| f.breed_ticks > 0).count() as f32 / n * 100.0,
            sum(&|f| f.matings as f32),
            sum(&|f| f.failed as f32),
            sum(&|f| f.kids as f32),
            sum(&|f| f.kids as f32) / n
        );
        println!(
            "  襲った {:.0} 回、仕留めた {:.0} 体（1,000 tick あたり 1 体 {:.2} 体）。最も近い仲間までの距離の平均 {:.0} px",
            sum(&|f| f.hunts as f32),
            sum(&|f| f.kills as f32),
            sum(&|f| f.kills as f32) / sum(&|f| f.ticks as f32).max(1.0) * 1000.0,
            sum(&|f| f.near_sum) / sum(&|f| f.ticks as f32).max(1.0)
        );
    }
}

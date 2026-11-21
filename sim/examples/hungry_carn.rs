//! 保存ファイル（圧縮なしの .lifesim）を読み込んで世界を少し進め、飢えかけた肉食がどう動いているかを測る。
//!
//! 肉食を、始めのエネルギー（繁殖に必要な量に対する割合）で分け、それぞれについて
//! 同じ向きに回った周数、道のり・位置の差、獲物・死体・覚えた場所が見えていた割合、
//! 欲求の優先度（食べる・自由）、自由な移動の前・右の成分、速さを比べる。
//! 獲物も死体も見えないと、食べる欲求の方向は (0, 0) になり、動きは自由な移動の向きで決まる。
//!
//! 使い方:
//!   cargo run --release --example hungry_carn --manifest-path sim/Cargo.toml -- world.lifesim [tick 数（既定 300）]

use sim::genome::{self, softsign};
use sim::world::{CARNIVORE, World};
use std::collections::HashMap;
use std::f32::consts::TAU;

#[derive(Default)]
struct Track {
    energy0: f32,
    last_h: (f32, f32),
    start: (f32, f32),
    last: (f32, f32),
    turn: f32,
    path: f32,
    n: u32,
    // 1 tick ごとの合計（あとで tick 数で割る）
    prey: f32,
    carrion: f32,
    mem: f32,
    mem_dist: f32,
    p_food: f32,
    p_free: f32,
    free_f: f32,
    free_r: f32,
    mem_f: f32,
    throttle: f32,
    // 獲物も死体も見えていない tick だけの合計
    blind: f32,
    blind_turn: f32,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u32 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(300);
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let (mut w, _) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  肉食 {}  （{ticks} tick 進めて測る）", w.tick, w.count_kind(CARNIVORE));

    let (width, height) = (w.width, w.height);
    let wrap = |d: f32, size: f32| d - size * (d / size).round();
    let inputs = w.brain_inputs();
    let mut tracks: HashMap<u32, Track> = HashMap::new();
    for i in 0..w.count() {
        if w.kind[i] == CARNIVORE {
            tracks.insert(
                w.id[i],
                Track {
                    energy0: inputs[i][genome::IN_ENERGY],
                    last_h: (w.hx[i], w.hy[i]),
                    start: (w.xs[i], w.ys[i]),
                    last: (w.xs[i], w.ys[i]),
                    ..Default::default()
                },
            );
        }
    }
    for _ in 0..ticks {
        let inputs = w.brain_inputs();
        let mut blind_now = HashMap::new();
        for i in 0..w.count() {
            let Some(t) = tracks.get_mut(&w.id[i]) else { continue };
            let x = &inputs[i];
            let out = genome::think(w.genome(i), x);
            let p = genome::priorities(&out);
            let prey = x[genome::IN_PREY_NEAR] > 0.0;
            let carrion = x[genome::IN_CARRION_FWD] != 0.0 || x[genome::IN_CARRION_RIGHT] != 0.0;
            t.prey += prey as u8 as f32;
            t.carrion += carrion as u8 as f32;
            t.mem += (x[genome::IN_MEM_VALUE] > 0.0) as u8 as f32;
            if x[genome::IN_MEM_VALUE] > 0.0 {
                let m = w.memory[i];
                let (dx, dy) = (wrap(m.x - w.xs[i], width), wrap(m.y - w.ys[i], height));
                t.mem_dist += (dx * dx + dy * dy).sqrt();
                t.mem_f += x[genome::IN_MEM_FWD];
            }
            t.p_food += p[0];
            t.p_free += p[5];
            t.free_f += softsign(out[genome::OUT_DIR_FWD]);
            t.free_r += softsign(out[genome::OUT_DIR_RIGHT]);
            t.throttle += 0.5 + 0.5 * softsign(out[genome::OUT_THROTTLE]);
            blind_now.insert(w.id[i], !prey && !carrion);
        }
        w.step();
        let mut alive = std::collections::HashSet::new();
        for i in 0..w.count() {
            let Some(t) = tracks.get_mut(&w.id[i]) else { continue };
            alive.insert(w.id[i]);
            let (ax, ay) = t.last_h;
            let (bx, by) = (w.hx[i], w.hy[i]);
            let d_turn = (ax * by - ay * bx).atan2(ax * bx + ay * by);
            t.turn += d_turn;
            if blind_now.get(&w.id[i]).copied().unwrap_or(false) {
                t.blind += 1.0;
                t.blind_turn += d_turn.abs();
            }
            t.last_h = (bx, by);
            let (dx, dy) = (wrap(w.xs[i] - t.last.0, width), wrap(w.ys[i] - t.last.1, height));
            t.path += (dx * dx + dy * dy).sqrt();
            t.last = (w.xs[i], w.ys[i]);
            t.n += 1;
        }
        tracks.retain(|id, _| alive.contains(id));
    }

    let k = ticks as f32;
    let disp = |t: &Track| {
        let (dx, dy) = (wrap(t.last.0 - t.start.0, width), wrap(t.last.1 - t.start.1, height));
        (dx * dx + dy * dy).sqrt()
    };
    println!("\n（エネルギーは始めの値。繁殖に必要な量に対する割合）");
    for (label, lo, hi) in [("飢えかけ（0.3 未満）", -1.0, 0.3), ("ふつう（0.3〜0.7）", 0.3, 0.7), ("満ち足りた（0.7 以上）", 0.7, 99.0)] {
        let g: Vec<&Track> = tracks.values().filter(|t| t.n == ticks && t.energy0 >= lo && t.energy0 < hi).collect();
        let m = g.len().max(1) as f32;
        let mean = |v: &dyn Fn(&Track) -> f32| g.iter().map(|t| v(t)).sum::<f32>() / m;
        let circling = g.iter().filter(|t| (t.turn / TAU).abs() >= 3.0 && disp(t) < 0.2 * t.path.max(1e-3)).count() as f32 / m * 100.0;
        println!("\n{label}: 最後まで生きた {} 体", g.len());
        println!(
            "  回り続けている {circling:.0}%、同じ向きに回った周数の平均 {:.1}、道のり {:.0} px、位置の差 {:.0} px",
            mean(&|t| (t.turn / TAU).abs()),
            mean(&|t| t.path),
            mean(&disp)
        );
        println!(
            "  見えていた割合: 獲物 {:.0}%、死体 {:.0}%、覚えた場所あり {:.0}%（覚えた場所までの距離 {:.0} px、その方向の前の成分 {:+.2}）",
            mean(&|t| t.prey / k) * 100.0,
            mean(&|t| t.carrion / k) * 100.0,
            mean(&|t| t.mem / k) * 100.0,
            mean(&|t| if t.mem > 0.0 { t.mem_dist / t.mem } else { 0.0 }),
            mean(&|t| if t.mem > 0.0 { t.mem_f / t.mem } else { 0.0 }),
        );
        println!(
            "  優先度: 食べる {:.2}、自由 {:.2}。自由な移動 前 {:+.2} 右 {:+.2}（右の絶対値 {:.2}）。速さ {:.2}",
            mean(&|t| t.p_food / k),
            mean(&|t| t.p_free / k),
            mean(&|t| t.free_f / k),
            mean(&|t| t.free_r / k),
            mean(&|t| (t.free_r / k).abs()),
            mean(&|t| t.throttle / k),
        );
        println!(
            "  獲物も死体も見えていない tick {:.0}%、そのときの 1 tick に曲がる角度 {:.3} rad",
            mean(&|t| t.blind / k) * 100.0,
            mean(&|t| if t.blind > 0.0 { t.blind_turn / t.blind } else { 0.0 }),
        );
    }
}

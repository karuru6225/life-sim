//! 保存ファイル（圧縮なしの .lifesim）を読み込んで世界を少し進め、個体が「その場で回り続けている」かを測る。
//!
//! 個体ごとに、向きの変化の累計（右回りが正、何周したか）、進んだ道のりと始めと終わりの位置の差、
//! 脳の「自由な移動」の出力の前・右の成分を記録する。回り続ける個体の曲がりが、自由な移動の右の成分で説明できるかを見る
//! （進む方向は自分基準の前・右で決まるので、右の成分が 0 でないと毎 tick 同じ向きに曲がり続ける）。
//!
//! 使い方:
//!   cargo run --release --features parallel --example circling --manifest-path sim/Cargo.toml -- world.lifesim [tick 数（既定 300）]

use sim::genome::{self, softsign};
use sim::world::{CARNIVORE, HERBIVORE, World};
use std::collections::HashMap;
use std::f32::consts::TAU;

#[derive(Default)]
struct Track {
    kind: u8,
    last_h: (f32, f32),
    start: (f32, f32),
    last: (f32, f32),
    turn: f32,
    path: f32,
    free_f: f32,
    free_r: f32,
    plant: f32,
    n: u32,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let ticks: u32 = args.next().map(|s| s.parse().expect("tick 数が数値ではない")).unwrap_or(300);
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let (mut w, _) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}  （{ticks} tick 進めて測る）", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));

    let (width, height) = (w.width, w.height);
    let wrap = |d: f32, size: f32| d - size * (d / size).round();
    let mut tracks: HashMap<u32, Track> = HashMap::new();
    for i in 0..w.count() {
        tracks.insert(
            w.id[i],
            Track { kind: w.kind[i], last_h: (w.hx[i], w.hy[i]), start: (w.xs[i], w.ys[i]), last: (w.xs[i], w.ys[i]), ..Default::default() },
        );
    }
    for _ in 0..ticks {
        // この tick の脳の出力（進める前の状態から。step の中の判断と同じ入力）
        let inputs = w.brain_inputs();
        for i in 0..w.count() {
            let Some(t) = tracks.get_mut(&w.id[i]) else { continue };
            let out = genome::think(w.genome(i), &inputs[i]);
            let p_free = genome::priorities(&out)[5];
            t.free_f += p_free * softsign(out[genome::OUT_DIR_FWD]);
            t.free_r += p_free * softsign(out[genome::OUT_DIR_RIGHT]);
            t.plant += inputs[i][genome::IN_PLANT_HERE];
        }
        w.step();
        let mut alive = std::collections::HashSet::new();
        for i in 0..w.count() {
            let Some(t) = tracks.get_mut(&w.id[i]) else { continue };
            alive.insert(w.id[i]);
            let (ax, ay) = t.last_h;
            let (bx, by) = (w.hx[i], w.hy[i]);
            // 右（自分基準の (−hy, hx) の向き）へ曲がると正
            t.turn += (ax * by - ay * bx).atan2(ax * bx + ay * by);
            t.last_h = (bx, by);
            let (dx, dy) = (wrap(w.xs[i] - t.last.0, width), wrap(w.ys[i] - t.last.1, height));
            t.path += (dx * dx + dy * dy).sqrt();
            t.last = (w.xs[i], w.ys[i]);
            t.n += 1;
        }
        // 途中で死んだ個体は外す（最後まで生きた個体だけを数える）
        tracks.retain(|id, _| alive.contains(id));
    }

    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let ts: Vec<&Track> = tracks.values().filter(|t| t.kind == kind && t.n == ticks).collect();
        let n = ts.len().max(1) as f64;
        let laps = |t: &Track| t.turn / TAU;
        let disp = |t: &Track| {
            let (dx, dy) = (wrap(t.last.0 - t.start.0, width), wrap(t.last.1 - t.start.1, height));
            (dx * dx + dy * dy).sqrt()
        };
        let pct = |f: &dyn Fn(&Track) -> bool| ts.iter().filter(|t| f(t)).count() as f64 / n * 100.0;
        println!("\n{name}（最後まで生きた {} 体）", ts.len());
        println!(
            "  {ticks} tick で同じ向きに回った周数: 1 周未満 {:.0}% / 1〜3 周 {:.0}% / 3〜10 周 {:.0}% / 10 周以上 {:.0}%",
            pct(&|t| laps(t).abs() < 1.0),
            pct(&|t| (1.0..3.0).contains(&laps(t).abs())),
            pct(&|t| (3.0..10.0).contains(&laps(t).abs())),
            pct(&|t| laps(t).abs() >= 10.0),
        );
        // 回り続けている: 3 周以上回り、始めと終わりの位置の差が道のりの 2 割未満
        let circling = |t: &Track| laps(t).abs() >= 3.0 && disp(t) < 0.2 * t.path.max(1e-3);
        let groups: [(&str, Box<dyn Fn(&Track) -> bool>); 2] =
            [("回り続けている", Box::new(circling)), ("それ以外", Box::new(move |t: &Track| !circling(t)))];
        for (label, f) in groups.iter() {
            let g: Vec<&&Track> = ts.iter().filter(|t| f(t)).collect();
            let m = g.len().max(1) as f32;
            let mean = |v: &dyn Fn(&Track) -> f32| g.iter().map(|t| v(t)).sum::<f32>() / m;
            let k = ticks as f32;
            // 自由な移動の成分だけで決まるとしたときの、1 tick に曲がる角度（右が正）と、実際に曲がった角度（1 tick あたり）
            let same_sign = g.iter().filter(|t| t.turn * t.free_r > 0.0).count() as f32 / m * 100.0;
            let free_ratio = mean(&|t| (t.free_r.abs() / k) / ((t.free_f / k).abs() + (t.free_r / k).abs()).max(1e-6));
            println!(
                "  {label} {:.0}%: 1 tick に曲がる角度 {:.3} rad、道のり {:.1} px（位置の差 {:.1} px）、足元の食べ物 {:.2}、\
                 自由な移動 前 {:+.3} 右 {:+.3}（右の割合 {:.2}）、回る向きと右の成分の符号が合う {:.0}%",
                g.len() as f32 / n as f32 * 100.0,
                mean(&|t| t.turn.abs() / k),
                mean(&|t| t.path),
                mean(&disp),
                mean(&|t| t.plant / k),
                mean(&|t| t.free_f / k),
                mean(&|t| t.free_r / k),
                free_ratio,
                same_sign,
            );
        }
    }
}

//! 保存ファイル（圧縮なしの .lifesim）を読み込み、草食が天敵から逃げるように進化しているかを測る。世界は進めない。
//!
//! - 草食の死因の内訳（世界を作ってからの累計）: 捕食がどれだけの割合を占めるか
//! - 天敵への反応: 各草食の今の入力に「真正面、視野の半分の距離に天敵」を書き込み、
//!   欲求の優先度（逃げる・食べる）と速さがどう変わるかを、進化した脳と初期の脳で比べる
//!
//! 使い方:
//!   cargo run --release --example flee_use --manifest-path sim/Cargo.toml -- world.lifesim

use sim::genome::{self, N_IN, OUT_THROTTLE, softsign};
use sim::world::{CARNIVORE, HERBIVORE, World};

const CAUSES: [&str; 8] = ["?", "餓死", "老衰", "食べられた", "反撃でけが", "病気", "敵との争い", "仲間との争い"];

fn main() {
    let path = std::env::args().nth(1).expect("保存ファイルを指定する");
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    let payload = strip_container(&data);
    let (mut w, _) = World::load(payload).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    println!("tick {}  草食 {}  肉食 {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE));

    // 死因の内訳
    let deaths = w.census.deaths[HERBIVORE as usize];
    let juvenile = w.census.juvenile_deaths[HERBIVORE as usize];
    let total: u64 = deaths.iter().sum();
    println!("\n草食の死因（累計 {total} 体、カッコ内は成熟前に死んだ数の割合）");
    for (c, name) in CAUSES.iter().enumerate().skip(1) {
        if deaths[c] > 0 {
            println!(
                "  {name:<8} {:>5.1}%（成熟前 {:>4.1}%）",
                deaths[c] as f64 / total.max(1) as f64 * 100.0,
                juvenile[c] as f64 / deaths[c] as f64 * 100.0
            );
        }
    }

    // 狩りの集計（世界を作ってからの累計）
    let c = &w.census;
    let hunts = c.hunts.max(1) as f64;
    println!(
        "
狩り: 成功 {:.1}%、襲われた獲物が遠ざかっていた速さ ÷ 肉食の最高速度 の平均 {:.3}",
        c.kills as f64 / hunts * 100.0,
        c.hunt_escape / hunts
    );

    // 天敵への反応
    let inputs = w.brain_inputs();
    let herbs: Vec<usize> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).collect();
    let seeing = herbs.iter().filter(|&&i| inputs[i][genome::IN_PRED_NEAR] > 0.0).count();
    println!("\n今、天敵が見えている草食 {:.1}%", seeing as f64 / herbs.len().max(1) as f64 * 100.0);

    // [逃げる優先度, 食べる優先度, 逃げる ÷ 優先度の合計, 速さ, 逃げる > 食べる の個体]
    let measure = |g: &[f32], input: &[f32; N_IN]| {
        let out = genome::think(g, input);
        let p = genome::priorities(&out);
        let sum: f32 = p.iter().sum();
        let throttle = 0.5 + 0.5 * softsign(out[OUT_THROTTLE]);
        [p[2], p[0], p[2] / sum.max(1e-6), throttle, (p[2] > p[0]) as u8 as f32]
    };
    let mut sums = [[0.0f64; 5]; 4]; // [進化/初期][天敵なし/あり]
    for &i in &herbs {
        let mut calm = inputs[i];
        for k in [genome::IN_PRED_FWD, genome::IN_PRED_RIGHT, genome::IN_PRED_NEAR, genome::IN_PRED_SIZE, genome::IN_KIN_ALERT] {
            calm[k] = 0.0;
        }
        let mut threat = calm;
        threat[genome::IN_PRED_FWD] = 1.0;
        threat[genome::IN_PRED_NEAR] = 0.75;
        threat[genome::IN_PRED_SIZE] = 1.5;
        let t = &w.traits[i];
        let initial = genome::initial(true, t.adult_size, t.max_speed, t.vision, 0.0);
        for (k, (g, input)) in [(w.genome(i), &calm), (w.genome(i), &threat), (&initial[..], &calm), (&initial[..], &threat)]
            .into_iter()
            .enumerate()
        {
            for (s, v) in sums[k].iter_mut().zip(measure(g, input)) {
                *s += v as f64;
            }
        }
    }
    let n = herbs.len().max(1) as f64;
    println!("\n天敵（真正面、視野の半分の距離）が見えたときの反応（草食の平均）");
    println!("                    逃げる優先度   食べる優先度   逃げるの占める割合   速さ   逃げる>食べる の個体");
    for (k, name) in ["進化した脳・天敵なし", "進化した脳・天敵あり", "初期の脳・天敵なし", "初期の脳・天敵あり"].iter().enumerate() {
        let m = sums[k].map(|v| v / n);
        println!(
            "  {name:<12} {:>10.2} {:>13.2} {:>17.0}% {:>9.2} {:>16.0}%",
            m[0],
            m[1],
            m[2] * 100.0,
            m[3],
            m[4] * 100.0
        );
    }
}

/// 保存ファイルの見出しを読み飛ばし、世界のデータの部分を返す
fn strip_container(data: &[u8]) -> &[u8] {
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = String::from_utf8_lossy(&data[12..12 + len]);
    println!("見出し: {header}");
    &data[12 + len..]
}

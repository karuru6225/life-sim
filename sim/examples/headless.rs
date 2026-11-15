//! 画面なしでシミュレーションを回し、個体数・バランス・形質・群れの度合いの推移を表示する。調整用。
//!
//! 使い方:
//!   cargo run --release --features parallel --example headless --manifest-path sim/Cargo.toml -- [tick数] [名前=値 ...]
//! 例:
//!   cargo run --release --features parallel --example headless --manifest-path sim/Cargo.toml -- 30000 sunlight=0.1 seed=3 every=1000
//!
//! save=ファイル名 を付けると、最後にブラウザで読み込める保存ファイル（圧縮なし）を書き出す。
//! 例: ... -- 50000 seed=3 save=world.lifesim  → ブラウザの「読み込み」で world.lifesim を選ぶと、5 万 tick から続きを見られる。
//! パラメータは保存ファイルに入るので、ブラウザでも同じ設定で続く
//!
//! 列の意味:
//!   量比: 肉食の量 ÷ 草食の量（量 = 体の大きさの合計）。現実の哺乳類では約 1/111
//!   植→草, 草→肉: 区間内の栄養段階の受け渡し効率（草食が得た量 ÷ 植物が日射から得た量、
//!                 肉食が得た量 ÷ 草食が得た量）。現実では平均 10% 前後
//!   警戒: 草食の警戒の度合いの平均（0〜1）
//!   集まり: 集まり具合（1 でランダム、小さいほど集まっている）
//!   そろい: 近くの仲間との向きのそろい具合（0 でばらばら、1 で同じ向き）
//!   同系/他系: そろい具合を、同じ系統どうし・違う系統どうしに分けたもの
//!   失敗: 区間内で、交配が相性で失敗した割合
//!   肉食の集まり・そろい・同系: 肉食の群れの度合い（草食と同じ指標）
//!   寒/暖: 草食を今いる場所の気温で寒い半分・暖かい半分に分けたときの、大人の大きさの平均（ベルクマンの法則の確認用）
//!   適温: 草食の好みの気温の平均
//!   期内: 区間内の交配のうち、日射の多い半年（季節 > 0）に行われた割合。0.5 なら季節と無関係
//!   信号: 繁殖シグナルの平均（草食/肉食）
//!   脳: 使っている中間ニューロンの数の平均（草食/肉食、1 層目と 2 層目の合計。初期は 8）
//!   投資/産数: 子への投資と一度に産む数の平均（草食; 肉食）
//!   大ばら: 大人の大きさの標準偏差（草食/肉食）。個体の大きさがどれだけばらけているか

use sim::params::Params;
use sim::world::{CARNIVORE, HERBIVORE, World};
use std::time::Instant;

fn main() {
    let mut ticks: u64 = 20_000;
    let mut report_every: u64 = 1_000;
    let mut params = Params::default();
    let mut seed = 1;
    let mut save_path: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if let Some((name, value)) = arg.split_once('=') {
            if name == "save" {
                save_path = Some(value.to_string());
                continue;
            }
            let value: f32 = value.parse().expect("値が数値ではない");
            match name {
                "seed" => seed = value as u64,
                "every" => report_every = value as u64,
                _ if !params.set(name, value) => panic!("知らないパラメータ: {name}"),
                _ => {}
            }
        } else {
            ticks = arg.parse().expect("tick 数が数値ではない");
        }
    }

    let mut w = World::new(seed, params);
    println!(
        "{:>7} {:>6} {:>5} {:>7} | {:>6} {:>5} {:>5} | 草食: {:>5} {:>5} {:>5} {:>4} {:>5} | {:>5} {:>5} {:>5} {:>5} | 肉食: {:>5} {:>5} {:>5} {:>5} {:>5} | {:>5} | {:>9} {:>5} | {:>5} {:>9} {:>11} {:>21} {:>9} | {:>6}",
        "tick", "草食", "肉食", "植物量", "量比", "植→草", "草→肉", "大きさ", "速度", "視野", "世代", "警戒", "集まり", "そろい", "同系", "他系", "大きさ", "速度", "集まり", "そろい", "同系", "失敗", "寒/暖", "適温", "期内", "信号", "脳", "投資/産数", "大ばら", "ms/tick"
    );
    let mut t = Instant::now();
    let mut last = w.ledger.clone();
    let (mut last_matings, mut last_failed) = (0u64, 0u64);
    // 交配が日射の多い半年に行われた回数と、全体の回数（区間内）
    let (mut in_season, mut all_season, mut prev_matings) = (0u64, 0u64, 0u64);
    for tick in 1..=ticks {
        let summer = w.season() > 0.0;
        w.step();
        let d = w.matings - prev_matings;
        prev_matings = w.matings;
        all_season += d;
        if summer {
            in_season += d;
        }
        if tick % report_every == 0 {
            let ms = t.elapsed().as_secs_f64() * 1000.0 / report_every as f64;
            let h = w.trait_means(HERBIVORE);
            let c = w.trait_means(CARNIVORE);
            let hh = w.herd_stats(HERBIVORE);
            let ch = w.herd_stats(CARNIVORE);
            let ratio = w.biomass(CARNIVORE) / w.biomass(HERBIVORE).max(1e-9);
            let l = &w.ledger;
            let plant_to_herb = (l.herb_gain - last.herb_gain) / (l.sunlight_in - last.sunlight_in).max(1e-9);
            let herb_to_carn = (l.carn_gain - last.carn_gain) / (l.herb_gain - last.herb_gain).max(1e-9);
            last = l.clone();
            let vig = {
                let v: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).map(|i| w.vigilance[i]).collect();
                v.iter().sum::<f32>() / v.len().max(1) as f32
            };
            let matings = w.matings - last_matings;
            let failed = w.failed_matings - last_failed;
            (last_matings, last_failed) = (w.matings, w.failed_matings);
            let fail = if matings > 0 { failed as f64 / matings as f64 } else { 0.0 };
            // 草食を今いる場所の気温で寒い半分・暖かい半分に分け、大人の大きさの平均を比べる
            let mut herbs: Vec<(f32, f32, f32)> = (0..w.count())
                .filter(|&i| w.kind[i] == HERBIVORE)
                .map(|i| (w.terrain.temperature_at(w.xs[i], w.ys[i]), w.traits[i].adult_size, w.traits[i].temp_pref))
                .collect();
            herbs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let half = herbs.len() / 2;
            let mean = |s: &[(f32, f32, f32)]| s.iter().map(|v| v.1).sum::<f32>() / s.len().max(1) as f32;
            let (cold, warm) = (mean(&herbs[..half]), mean(&herbs[half..]));
            let pref = herbs.iter().map(|v| v.2).sum::<f32>() / herbs.len().max(1) as f32;
            let season_share = in_season as f64 / all_season.max(1) as f64;
            (in_season, all_season) = (0, 0);
            let size_sd = |kind: u8| {
                let v: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.traits[i].adult_size).collect();
                let n = v.len().max(1) as f32;
                let m = v.iter().sum::<f32>() / n;
                (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / n).sqrt()
            };
            let litter = |kind: u8| {
                let v: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.traits[i].litter).collect();
                v.iter().sum::<f32>() / v.len().max(1) as f32
            };
            let brain = |kind: u8| {
                let v: Vec<u32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.traits[i].brain.0 + w.traits[i].brain.1).collect();
                v.iter().sum::<u32>() as f32 / v.len().max(1) as f32
            };
            let signal = |kind: u8| {
                let v: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.signal[i]).collect();
                v.iter().sum::<f32>() / v.len().max(1) as f32
            };
            println!(
                "{tick:>7} {:>6} {:>5} {:>7.0} | 1/{:<4.0} {:>5.3} {:>5.3} |       {:>5.2} {:>5.2} {:>5.1} {:>4.0} {:>5.2} | {:>5.2} {:>5.2} {:>5.2} {:>5.2} |       {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2} | {:>5.3} | {:>4.2}/{:<4.2} {:>5.1} | {:>5.2} {:>4.2}/{:<4.2} {:>5.2}/{:<5.2} {:>4.2}/{:<4.2}; {:>4.2}/{:<4.2} {:>4.2}/{:<4.2} | {ms:>6.2}",
                h[0], c[0], w.plants.total(), 1.0 / ratio.max(1e-9), plant_to_herb, herb_to_carn,
                h[1], h[2], h[3], h[5], vig, hh[0], hh[1], hh[2], hh[3], c[1], c[2], ch[0], ch[1], ch[2], fail, cold, warm, pref,
                season_share, signal(HERBIVORE), signal(CARNIVORE), brain(HERBIVORE), brain(CARNIVORE),
                h[4], litter(HERBIVORE), c[4], litter(CARNIVORE),
                size_sd(HERBIVORE), size_sd(CARNIVORE)
            );
            if w.count_kind(HERBIVORE) == 0 && w.count_kind(CARNIVORE) == 0 {
                println!("全滅");
                break;
            }
            t = Instant::now();
        }
    }
    let err = w.ledger.expected_total() - w.total_energy();
    println!("エネルギー帳尻の誤差: {err:.4}（帳簿 {:.1}）", w.ledger.expected_total());
    if let Some(path) = save_path {
        let data = sim::world::save_file(&w, &commit_hash(), &now_iso());
        std::fs::write(&path, &data).expect("保存ファイルを書けない");
        println!("tick {} の世界を {path} に保存した（{:.1} MB）。ブラウザの「読み込み」で開ける", w.tick, data.len() as f64 / 1e6);
    }
}

/// 今のコードのコミット（短いハッシュ。未コミットの変更があれば -dirty）。ブラウザのビルドと同じ書き方
fn commit_hash() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    match git(&["rev-parse", "--short", "HEAD"]) {
        Some(hash) if !hash.is_empty() => {
            let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
            if dirty { format!("{hash}-dirty") } else { hash }
        }
        _ => "unknown".to_string(),
    }
}

/// 今の日時（UTC、ISO 8601）
fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // 1970-01-01 からの日数を年月日に直す（Howard Hinnant の civil_from_days）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

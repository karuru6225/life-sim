//! 保存ファイル（圧縮なしの .lifesim）を読み込み、脳の配線が初期個体からどう変わったかを測る。世界は進めない。
//!
//! 初期個体は、保存ファイルのシードとパラメータで World::new を作り直して得る（今のコードの初期配線で作るので、
//! 保存したときのコードと初期配線が同じであることが前提）。
//! 重みは「効いている重み」で比べる。つながる中間ニューロンを使っていなければ 0 とみなす
//! （使わないニューロンの重みは変異せず残っているが、出力には効かない）。
//!
//! 出すもの（種類ごと）:
//! - 中間ニューロンを使っている個体の割合（初期 → 今）
//! - 層ごとの変化の大きさ
//! - 平均の変化が大きい重みの一覧（初期の平均 → 今の平均、今のばらつき）
//! - 初期に使っていなかったのに今は多くの個体が使っているニューロンの、主な入力と出力
//!
//! 使い方（ブラウザの .lifesim.gz は gzip -dc で解いて渡す）:
//!   cargo run --release --example brain_drift --manifest-path sim/Cargo.toml -- world.lifesim [一覧の件数]

use sim::genome::{self, GENOME_LEN, H1, H2, N_HIDDEN, N_IN, N_OUT, N_TRAITS};
use sim::world::{CARNIVORE, HERBIVORE, World};

// genome.rs と同じ遺伝子の並び
const ACT: usize = N_TRAITS;
const W1: usize = ACT + N_HIDDEN;
const W1_ROW: usize = N_IN + 1;
const W2: usize = W1 + H1 * W1_ROW;
const W2_ROW: usize = H1 + 1;
const W3: usize = W2 + H2 * W2_ROW;
const W3_ROW: usize = N_HIDDEN + 1;

const IN_NAMES: [&str; N_IN] = [
    "エネルギー",
    "足元の草",
    "草の勾配・前",
    "草の勾配・右",
    "天敵・前",
    "天敵・右",
    "天敵の近さ",
    "獲物・前",
    "獲物・右",
    "獲物の近さ",
    "仲間・前",
    "仲間・右",
    "仲間の近さ",
    "仲間の数",
    "仲間の向き・前",
    "仲間の向き・右",
    "仲間の興奮",
    "近縁・前",
    "近縁・右",
    "近縁の割合",
    "繁殖できる",
    "相手の近さ",
    "前方が水",
    "獲物の大きさ比",
    "天敵の大きさ比",
    "植生の合い具合",
    "気温の差",
    "季節",
    "仲間の繁殖シグナル",
    "呼び声・前",
    "呼び声・右",
    "呼び声の大きさ",
    "死角の見張り",
    "足元の死体",
    "死体・前",
    "死体・右",
    "死体の新しさ",
    "敵・前",
    "敵・右",
    "敵の近さ",
    "敵の大きさ比",
    "敵の数",
    "攻撃された",
    "一人でいる",
    "食べ物の乏しさ",
    "遠くの景色・前",
    "遠くの景色・右",
    "記憶・前",
    "記憶・右",
    "記憶の良さ",
    "巣・前",
    "巣・右",
    "足元の巣",
    "巣から離れている",
    "体力",
    "親子・前",
    "親子・右",
    "親子の近さ",
    "成長度",
    "子・前",
    "子・右",
    "子の近さ",
    "子の空腹",
    "産んだ場所・前",
    "産んだ場所・右",
    "妊娠",
    "獲物が声を出す",
];

const OUT_NAMES: [&str; N_OUT] = [
    "優先:食べる",
    "優先:繁殖",
    "優先:逃げる",
    "優先:仲間",
    "優先:自由",
    "自由な向き・前",
    "自由な向き・右",
    "速さ",
    "繁殖する",
    "食べる",
    "警戒",
    "繁殖シグナル",
    "優先:追い払う",
    "攻撃",
    "仲間を攻撃",
    "巣を作る",
    "子に与える",
    "警戒の声",
];

fn hidden_name(h: usize) -> String {
    if h < H1 { format!("中間1-{h}") } else { format!("中間2-{}", h - H1) }
}

/// 遺伝子の位置 k（脳の重み）の名前と、効くために使っている必要がある中間ニューロン（最大 2 つ）
fn describe(k: usize) -> (String, [Option<usize>; 2]) {
    if k >= W3 {
        let (o, col) = ((k - W3) / W3_ROW, (k - W3) % W3_ROW);
        if col == N_HIDDEN {
            (format!("{} の基準値", OUT_NAMES[o]), [None, None])
        } else {
            (format!("{} → {}", hidden_name(col), OUT_NAMES[o]), [Some(col), None])
        }
    } else if k >= W2 {
        let (h, col) = ((k - W2) / W2_ROW, (k - W2) % W2_ROW);
        if col == H1 {
            (format!("{} の基準値", hidden_name(H1 + h)), [Some(H1 + h), None])
        } else {
            (format!("{} → {}", hidden_name(col), hidden_name(H1 + h)), [Some(H1 + h), Some(col)])
        }
    } else {
        let (h, col) = ((k - W1) / W1_ROW, (k - W1) % W1_ROW);
        if col == N_IN {
            (format!("{} の基準値", hidden_name(h)), [Some(h), None])
        } else {
            (format!("{} → {}", IN_NAMES[col], hidden_name(h)), [Some(h), None])
        }
    }
}

/// 効いている重み（つながる中間ニューロンを使っていなければ 0）
fn effective(g: &[f32], k: usize, needs: &[Option<usize>; 2]) -> f32 {
    if needs.iter().flatten().all(|&h| genome::neuron_active(g, h)) { g[k] } else { 0.0 }
}

struct Stats {
    mean: Vec<f64>,
    sd: Vec<f64>,
    active: Vec<f64>,
    /// 重みが効いている個体（つながるニューロンを使っている個体）だけでの平均。効いている個体がいなければ NaN
    mean_used: Vec<f64>,
}

fn stats(genomes: &[&[f32]]) -> Stats {
    let n = genomes.len().max(1) as f64;
    let len = GENOME_LEN - W1;
    let mut sum = vec![0.0f64; len];
    let mut sq = vec![0.0f64; len];
    let mut used_sum = vec![0.0f64; len];
    let mut used_n = vec![0usize; len];
    for g in genomes {
        for j in 0..len {
            let k = W1 + j;
            let (_, needs) = describe(k);
            let used = needs.iter().flatten().all(|&h| genome::neuron_active(g, h));
            let v = if used { g[k] as f64 } else { 0.0 };
            sum[j] += v;
            sq[j] += v * v;
            if used {
                used_sum[j] += v;
                used_n[j] += 1;
            }
        }
    }
    let mean: Vec<f64> = sum.iter().map(|s| s / n).collect();
    let sd = sq.iter().zip(&mean).map(|(q, m)| (q / n - m * m).max(0.0).sqrt()).collect();
    let active = (0..N_HIDDEN).map(|h| genomes.iter().filter(|g| genome::neuron_active(g, h)).count() as f64 / n).collect();
    let mean_used = used_sum.iter().zip(&used_n).map(|(s, &c)| if c > 0 { s / c as f64 } else { f64::NAN }).collect();
    Stats { mean, sd, active, mean_used }
}

fn main() {
    assert_eq!(W3 + N_OUT * W3_ROW, GENOME_LEN, "遺伝子の並びが genome.rs と合わない");
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("保存ファイルを指定する");
    let top: usize = args.next().map(|s| s.parse().expect("件数は整数")).unwrap_or(30);
    let data = std::fs::read(&path).expect("保存ファイルを読めない");
    assert!(data.starts_with(b"LIFESIM\n"), "life-sim の保存ファイル（圧縮なし）ではない");
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    println!("見出し: {}", String::from_utf8_lossy(&data[12..12 + len]));
    let (w, unknown) = World::load(&data[12 + len..]).unwrap_or_else(|e| panic!("読み込めない: {e}"));
    if !unknown.is_empty() {
        println!("今のコードが知らないパラメータ: {unknown:?}");
    }
    println!("tick {}  草食 {}  肉食 {}  シード {}", w.tick, w.count_kind(HERBIVORE), w.count_kind(CARNIVORE), w.seed);
    let founders = World::new(w.seed, w.params.clone());
    let sigma = w.params.mutation_weight_sigma;

    for (kind, name) in [(HERBIVORE, "草食"), (CARNIVORE, "肉食")] {
        let now: Vec<&[f32]> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.genome(i)).collect();
        let init: Vec<&[f32]> = (0..founders.count()).filter(|&i| founders.kind[i] == kind).map(|i| founders.genome(i)).collect();
        if now.is_empty() {
            continue;
        }
        let gens: Vec<u32> = (0..w.count()).filter(|&i| w.kind[i] == kind).map(|i| w.generation[i]).collect();
        let (a, b) = (stats(&init), stats(&now));
        println!("\n==================== {name}（今 {} 体、初期 {} 体）====================", now.len(), init.len());
        println!(
            "世代: 平均 {:.0}、最大 {}　（重みの変異 1 回の大きさ σ = {sigma}）",
            gens.iter().map(|&g| g as f64).sum::<f64>() / gens.len() as f64,
            gens.iter().max().unwrap()
        );

        // 使っているニューロン
        println!("\n使っている中間ニューロン（使っている個体の割合 初期 → 今。どちらも 5% 未満は省く）");
        let (n1a, n2a): (f64, f64) = ((0..H1).map(|h| a.active[h]).sum(), (H1..N_HIDDEN).map(|h| a.active[h]).sum());
        let (n1b, n2b): (f64, f64) = ((0..H1).map(|h| b.active[h]).sum(), (H1..N_HIDDEN).map(|h| b.active[h]).sum());
        println!("  1 体あたりの数: 1 層目 {n1a:.1} → {n1b:.1}、2 層目 {n2a:.1} → {n2b:.1}");
        let mut line = String::new();
        for h in 0..N_HIDDEN {
            if a.active[h] < 0.05 && b.active[h] < 0.05 {
                continue;
            }
            line += &format!("  {:<8} {:>4.0}% → {:>4.0}%", hidden_name(h), a.active[h] * 100.0, b.active[h] * 100.0);
            line += if a.active[h] < 0.5 && b.active[h] >= 0.5 {
                "  ← 新しく使う\n"
            } else if a.active[h] >= 0.5 && b.active[h] < 0.5 {
                "  ← 使わなくなった\n"
            } else {
                "\n"
            };
        }
        print!("{line}");

        // 層ごとの変化
        let layer = |lo: usize, hi: usize| {
            let d: Vec<f64> = (lo..hi).map(|k| (b.mean[k - W1] - a.mean[k - W1]).abs()).collect();
            let big = d.iter().filter(|&&x| x > 0.5).count();
            (d.iter().sum::<f64>(), big)
        };
        println!("\n層ごとの平均の変化（|今の平均 − 初期の平均| の合計、そのうち 0.5 を超える重みの数）");
        for (label, lo, hi) in [("入力 → 1 層目", W1, W2), ("1 層目 → 2 層目", W2, W3), ("中間 → 出力", W3, GENOME_LEN)] {
            let (s, big) = layer(lo, hi);
            println!("  {label:<14} 合計 {s:>7.1}　0.5 超 {big} 本");
        }

        // 変化の大きい重み
        let mut order: Vec<usize> = (0..GENOME_LEN - W1).collect();
        order.sort_by(|&x, &y| (b.mean[y] - a.mean[y]).abs().total_cmp(&(b.mean[x] - a.mean[x]).abs()));
        println!("\n平均の変化が大きい重み（上位 {top}。効いている重みで、使っていないニューロンの分は 0 として平均）");
        println!("  「使う個体で」は、つながるニューロンを使っている個体だけでの今の平均（効いている個体がいなければ -）");
        println!("  {:<34} {:>8} {:>8} {:>8} {:>8} {:>10}", "重み", "初期", "今", "変化", "今のSD", "使う個体で");
        for &j in order.iter().take(top) {
            let (label, _) = describe(W1 + j);
            let used = if b.mean_used[j].is_nan() { "-".to_string() } else { format!("{:.2}", b.mean_used[j]) };
            println!(
                "  {label:<34} {:>8.2} {:>8.2} {:>+8.2} {:>8.2} {used:>10}",
                a.mean[j],
                b.mean[j],
                b.mean[j] - a.mean[j],
                b.sd[j]
            );
        }

        // 新しく使うようになったニューロンの配線
        for h in 0..N_HIDDEN {
            if !(a.active[h] < 0.5 && b.active[h] >= 0.5) {
                continue;
            }
            println!("\n{}（今 {:.0}% が使う）の主な配線（効いている重みの平均）", hidden_name(h), b.active[h] * 100.0);
            let mut ins: Vec<(String, f64)> = if h < H1 {
                (0..=N_IN).map(|c| (describe(W1 + h * W1_ROW + c).0, b.mean[h * W1_ROW + c])).collect()
            } else {
                let r = h - H1;
                (0..=H1).map(|c| (describe(W2 + r * W2_ROW + c).0, b.mean[W2 - W1 + r * W2_ROW + c])).collect()
            };
            ins.sort_by(|x, y| y.1.abs().total_cmp(&x.1.abs()));
            let outs: Vec<(String, f64)> = {
                let mut v: Vec<_> = (0..N_OUT).map(|o| (OUT_NAMES[o].to_string(), b.mean[W3 - W1 + o * W3_ROW + h])).collect();
                v.sort_by(|x, y| y.1.abs().total_cmp(&x.1.abs()));
                v
            };
            println!("  入る: {}", ins.iter().take(5).map(|(l, v)| format!("{l} {v:+.2}")).collect::<Vec<_>>().join(" / "));
            println!("  出る: {}", outs.iter().take(5).map(|(l, v)| format!("{l} {v:+.2}")).collect::<Vec<_>>().join(" / "));
        }
    }
}

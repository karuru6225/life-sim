//! 遺伝子と脳。
//!
//! 1 個体の遺伝子は f32 の並び（長さ GENOME_LEN）で、前半が体の形質、後半が脳の重み。
//! 全個体の遺伝子は 1 本の配列に GENOME_LEN ずつ詰めて持つ（SoA の一種）。
//!
//! 脳は 入力 N_IN → 中間 N_HIDDEN → 出力 N_OUT の 2 層ニューラルネット。
//! 活性化関数は softsign（x / (1 + |x|)）。tanh とほぼ同じ形で、指数関数を使わないので速い。

use crate::rng::Rng;

// ---- 体の形質（遺伝子の先頭） ----
pub const G_SIZE: usize = 0;
pub const G_SPEED: usize = 1;
pub const G_VISION: usize = 2;
pub const G_INVEST: usize = 3;
pub const G_HUE: usize = 4;
/// 好みの獲物の大きさ（獲物の大きさ ÷ 自分の大きさ）の下限と上限。肉食はこの範囲の獲物だけを狙う（草食は使わない）
pub const G_PREY_MIN: usize = 5;
pub const G_PREY_MAX: usize = 6;
pub const N_TRAITS: usize = 7;

/// 形質が取りうる範囲（変異でこの外に出たら切り詰める）
pub const SIZE_RANGE: (f32, f32) = (0.4, 3.0);
pub const SPEED_RANGE: (f32, f32) = (0.2, 4.0);
pub const VISION_RANGE: (f32, f32) = (5.0, VISION_MAX);
pub const INVEST_RANGE: (f32, f32) = (0.1, 0.9);
pub const PREY_MIN_RANGE: (f32, f32) = (0.05, 2.0);
pub const PREY_MAX_RANGE: (f32, f32) = (0.2, 4.0);
/// 視野の上限。近傍探索のグリッドのセルはこれ以上の大きさにする
pub const VISION_MAX: f32 = 64.0;

// ---- 脳の入力 ----
// 方向は自分の向きを基準にした単位ベクトルの (前方成分, 右方成分)。
// *_NEAR は、相手が見えていれば 0.5〜1（近いほど大きい）、見えていなければ 0。
// 何も見えていないときは方向も 0。
pub const IN_ENERGY: usize = 0;
pub const IN_PLANT_HERE: usize = 1;
pub const IN_GRAD_FWD: usize = 2;
pub const IN_GRAD_RIGHT: usize = 3;
pub const IN_PRED_FWD: usize = 4;
pub const IN_PRED_RIGHT: usize = 5;
pub const IN_PRED_NEAR: usize = 6;
pub const IN_PREY_FWD: usize = 7;
pub const IN_PREY_RIGHT: usize = 8;
pub const IN_PREY_NEAR: usize = 9;
pub const IN_KIN_FWD: usize = 10;
pub const IN_KIN_RIGHT: usize = 11;
pub const IN_KIN_NEAR: usize = 12;
// 群れの知覚（見えている仲間全体の集計）
/// 見えている仲間の数（softsign(数 / 4) で 0〜1 に縮める）
pub const IN_KIN_COUNT: usize = 13;
/// 見えている仲間の平均の向き（自分基準の前方・右方成分。向きがそろっているほど長い）
pub const IN_KIN_HEAD_FWD: usize = 14;
pub const IN_KIN_HEAD_RIGHT: usize = 15;
/// 見えている仲間のうち、興奮している（草食は天敵が、肉食は獲物が見えている）割合
pub const IN_KIN_ALERT: usize = 16;
// 系統の見分け（標識色が自分と近い仲間 ＝ 血縁の近い仲間）
/// 標識色が近い仲間のうち最寄りの方向（自分基準の前方・右方成分）
pub const IN_SIM_FWD: usize = 17;
pub const IN_SIM_RIGHT: usize = 18;
/// 見えている仲間のうち、標識色が近い個体の割合
pub const IN_SIM_RATIO: usize = 19;
/// 繁殖できる状態か（成熟していて、エネルギーが足りていれば 1）
pub const IN_READY: usize = 20;
/// 繁殖相手（成熟した仲間）が見えていれば 0.5〜1（近いほど大きい）、見えていなければ 0
pub const IN_MATE_NEAR: usize = 21;
/// 前方（WATER_LOOKAHEAD px 先）が水なら 1
pub const IN_WATER_AHEAD: usize = 22;
/// 最寄りの獲物の大きさ ÷ 自分の大きさ（見えていなければ 0）。大きすぎる獲物は反撃してくる
pub const IN_PREY_SIZE: usize = 23;
/// 最寄りの天敵の大きさ ÷ 自分の大きさ（見えていなければ 0）
pub const IN_PRED_SIZE: usize = 24;
/// 今いる場所の植生が、自分（草食）の食べられる植生にどれだけ合っているか（0〜1。肉食は常に 1）
pub const IN_HABITAT_FIT: usize = 25;
pub const N_IN: usize = 26;
/// 「前方が水か」を調べる距離（px）
pub const WATER_LOOKAHEAD: f32 = 12.0;

pub const N_HIDDEN: usize = 8;

// ---- 脳の出力 ----
// 進む方向は「欲求」ごとの方向を、脳が出す優先度で混ぜて決める（mix_drives）。
// 欲求ごとの方向そのもの（食べ物へ、相手へ、天敵と反対へ）は固定のルールで決まり、
// どの欲求をどれだけ優先するかが遺伝子で決まる。
// 「繁殖できるときだけ相手へ向かう」のような条件付きの行動は、小さなネットが変異だけで
// 方向まで含めて作り上げるのは難しいので、方向は固定し、優先度の付け方だけを進化に任せる。
/// 食べる欲求の優先度（草食は草の多い方へ、肉食は獲物へ）
pub const OUT_PRI_FOOD: usize = 0;
/// 繁殖の欲求の優先度（繁殖相手へ）
pub const OUT_PRI_MATE: usize = 1;
/// 逃げる欲求の優先度（天敵と反対へ）
pub const OUT_PRI_FLEE: usize = 2;
/// 仲間に寄る欲求の優先度（見えている仲間の中心へ）
pub const OUT_PRI_KIN: usize = 3;
/// 自由な移動の優先度（OUT_DIR_* の方向へ）。想定していない行動の余地
pub const OUT_PRI_FREE: usize = 4;
/// 自由な移動の方向（自分基準の前方成分・右方成分）
pub const OUT_DIR_FWD: usize = 5;
pub const OUT_DIR_RIGHT: usize = 6;
/// 速さ（最高速度に対する割合に変換して使う）
pub const OUT_THROTTLE: usize = 7;
/// 正なら繁殖する（エネルギーが足りていれば）
pub const OUT_REPRODUCE: usize = 8;
/// 正なら食べる（草食は目の前の草、肉食は届く範囲の獲物）
pub const OUT_EAT: usize = 9;
/// 警戒の度合い（草食のみ使う）。高いほど天敵に遠くから気づけるが、一口が小さくなる
pub const OUT_VIGILANCE: usize = 10;
pub const N_OUT: usize = 11;

/// 欲求の数（食べる・繁殖・逃げる・仲間・自由）。最後の「自由」の方向は脳の出力から取る
pub const N_DRIVES: usize = 5;

/// 出力から欲求ごとの優先度（0〜1）を取り出す
pub fn priorities(out: &[f32; N_OUT]) -> [f32; N_DRIVES] {
    [OUT_PRI_FOOD, OUT_PRI_MATE, OUT_PRI_FLEE, OUT_PRI_KIN, OUT_PRI_FREE].map(|k| 0.5 + 0.5 * softsign(out[k]))
}

/// 出力から警戒の度合い（0〜1）を取り出す
pub fn vigilance(out: &[f32; N_OUT]) -> f32 {
    0.5 + 0.5 * softsign(out[OUT_VIGILANCE])
}

/// 欲求ごとの方向 drives（自分基準の単位ベクトル。対象がいなければ (0, 0)）を、優先度で混ぜた方向。
/// 自由な移動の方向は脳の出力から取る（drives の最後の要素は無視する）
pub fn mix_drives(out: &[f32; N_OUT], drives: &[(f32, f32); N_DRIVES]) -> (f32, f32) {
    let p = priorities(out);
    let free = (softsign(out[OUT_DIR_FWD]), softsign(out[OUT_DIR_RIGHT]));
    let mut f = 0.0;
    let mut r = 0.0;
    for k in 0..N_DRIVES {
        let (df, dr) = if k == N_DRIVES - 1 { free } else { drives[k] };
        f += p[k] * df;
        r += p[k] * dr;
    }
    (f, r)
}

/// 中間層の重みの開始位置。1 ニューロンあたり N_IN 個の重み + バイアス 1 個
const W1: usize = N_TRAITS;
/// 出力層の重みの開始位置。1 出力あたり N_HIDDEN 個の重み + バイアス 1 個
const W2: usize = W1 + N_HIDDEN * (N_IN + 1);
pub const GENOME_LEN: usize = W2 + N_OUT * (N_HIDDEN + 1);

pub fn softsign(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

/// 脳を 1 回動かす
pub fn think(genome: &[f32], input: &[f32; N_IN]) -> [f32; N_OUT] {
    let mut hidden = [0.0f32; N_HIDDEN];
    for (h, out) in hidden.iter_mut().enumerate() {
        let w = &genome[W1 + h * (N_IN + 1)..W1 + (h + 1) * (N_IN + 1)];
        let mut sum = w[N_IN]; // バイアス
        for k in 0..N_IN {
            sum += w[k] * input[k];
        }
        *out = softsign(sum);
    }
    let mut output = [0.0f32; N_OUT];
    for (o, out) in output.iter_mut().enumerate() {
        let w = &genome[W2 + o * (N_HIDDEN + 1)..W2 + (o + 1) * (N_HIDDEN + 1)];
        let mut sum = w[N_HIDDEN];
        for k in 0..N_HIDDEN {
            sum += w[k] * hidden[k];
        }
        *out = sum;
    }
    output
}

/// 体の形質を範囲内に収めて読み出したもの。
/// 大きさは「遺伝子が決める大人の大きさ」と「成長度を掛けたいまの大きさ」に分けて持つ
#[derive(Clone, Copy, Debug)]
pub struct Traits {
    /// 遺伝子が決める大人の大きさ
    pub adult_size: f32,
    /// adult_size の平方根（寿命の計算用）
    pub adult_sqrt: f32,
    /// いまの大きさ（大人の大きさ × 成長度）。代謝・移動・一口の量・狩りに使う
    pub size: f32,
    /// いまの大きさの 0.75 乗（代謝の計算用。毎 tick 計算すると重いので先に求めておく）
    pub size_pow075: f32,
    /// 1 tick に曲がれる最大の角度の cos と sin（いまの大きさが大きいほど小回りが利かない）
    pub turn_cos: f32,
    pub turn_sin: f32,
    pub max_speed: f32,
    pub vision: f32,
    pub invest: f32,
    /// 好みの獲物の大きさの比の下限と上限（肉食のみ使う）
    pub prey_min: f32,
    pub prey_max: f32,
    /// 0〜1（色相）
    pub hue: f32,
}

impl Traits {
    /// 成長度 growth（0〜1）に合わせて、いまの大きさと、それから決まる値を計算し直す
    pub fn grow_to(&mut self, growth: f32, max_turn: f32) {
        self.size = self.adult_size * growth;
        self.size_pow075 = self.size.powf(0.75);
        let (turn_sin, turn_cos) = (max_turn / self.size.sqrt()).min(std::f32::consts::PI).sin_cos();
        self.turn_cos = turn_cos;
        self.turn_sin = turn_sin;
    }
}

/// 遺伝子から形質を読み出す（成長しきった大人として）。max_turn は大きさ 1 のときの最大旋回角（ラジアン）
pub fn traits(genome: &[f32], max_turn: f32) -> Traits {
    let adult_size = genome[G_SIZE].clamp(SIZE_RANGE.0, SIZE_RANGE.1);
    let mut t = Traits {
        adult_size,
        adult_sqrt: adult_size.sqrt(),
        size: adult_size,
        size_pow075: 0.0,
        turn_cos: 0.0,
        turn_sin: 0.0,
        max_speed: genome[G_SPEED].clamp(SPEED_RANGE.0, SPEED_RANGE.1),
        vision: genome[G_VISION].clamp(VISION_RANGE.0, VISION_RANGE.1),
        invest: genome[G_INVEST].clamp(INVEST_RANGE.0, INVEST_RANGE.1),
        prey_min: genome[G_PREY_MIN].clamp(PREY_MIN_RANGE.0, PREY_MIN_RANGE.1),
        prey_max: genome[G_PREY_MAX].clamp(PREY_MAX_RANGE.0, PREY_MAX_RANGE.1),
        hue: genome[G_HUE].rem_euclid(1.0),
    };
    t.grow_to(1.0, max_turn);
    t
}

/// 中間層ニューロン h を、入力 k の 1 本だけにつなぐ（手で配線するとき用）
fn wire_hidden(g: &mut [f32], h: usize, input: usize, weight: f32) {
    g[W1 + h * (N_IN + 1) + input] = weight;
}

fn wire_out(g: &mut [f32], o: usize, hidden: usize, weight: f32) {
    g[W2 + o * (N_HIDDEN + 1) + hidden] = weight;
}

fn hidden_bias(g: &mut [f32], h: usize, bias: f32) {
    g[W1 + h * (N_IN + 1) + N_IN] = bias;
}

fn out_bias(g: &mut [f32], o: usize, bias: f32) {
    g[W2 + o * (N_HIDDEN + 1) + N_HIDDEN] = bias;
}

/// 初期個体の遺伝子。脳は Phase 1 の手書きルールとほぼ同じ動きをするように配線してある。
/// 進化はここから始まる。
/// 中間層の各ニューロンは入力 1 本を受け取るだけ（ほぼ素通し）で、割り当ては種類ごとに違う
pub fn initial(herbivore: bool, size: f32, max_speed: f32, vision: f32, hue: f32) -> Vec<f32> {
    let mut g = vec![0.0; GENOME_LEN];
    g[G_SIZE] = size;
    g[G_SPEED] = max_speed;
    g[G_VISION] = vision;
    g[G_INVEST] = 0.5;
    g[G_HUE] = hue;
    // 自分の 0.4〜1.3 倍の獲物を狙う（小さすぎると割に合わず、大きすぎると狩れないうえ反撃を受ける）
    g[G_PREY_MIN] = 0.4;
    g[G_PREY_MAX] = 1.3;

    // 自由な移動は、ゆるく前へ
    out_bias(&mut g, OUT_DIR_FWD, 0.3);

    // 優先度の基本: 普段は食べる、繁殖できる状態なら繁殖、天敵が見えたら逃げるを最優先。
    // 仲間に寄る欲求は弱くしておき、強めるかどうかは進化に任せる
    let (h_threat, h_ready, h_plant, h_full, h_alarm) = (0, 1, 2, 3, 4);
    out_bias(&mut g, OUT_PRI_KIN, -4.0);
    // 満腹（蓄えが繁殖に必要な量の 1.5 倍を超えるあたりから）なら、食べるのを控える
    wire_hidden(&mut g, h_full, IN_ENERGY, 4.0);
    hidden_bias(&mut g, h_full, -6.0);
    wire_out(&mut g, OUT_PRI_FOOD, h_full, -3.0);
    wire_out(&mut g, OUT_EAT, h_full, -4.0);
    wire_hidden(&mut g, h_ready, IN_READY, 4.0);
    // 食べる優先度は空腹度で変わる（下の満腹の配線と合わせて、エネルギーが繁殖に必要な量の
    // 半分なら 0.8 前後、繁殖できる状態なら 0.3 前後、満腹なら 0.1 前後）
    out_bias(&mut g, OUT_PRI_FOOD, -1.0);
    out_bias(&mut g, OUT_PRI_MATE, -3.0);
    out_bias(&mut g, OUT_PRI_FLEE, -4.0);
    wire_out(&mut g, OUT_PRI_MATE, h_ready, 6.0);
    wire_out(&mut g, OUT_PRI_FOOD, h_ready, -2.0);

    if herbivore {
        // 天敵が見えたら、食べるのも相手探しもやめて逃げる
        wire_hidden(&mut g, h_threat, IN_PRED_NEAR, 4.0);
        wire_out(&mut g, OUT_PRI_FLEE, h_threat, 10.0);
        // 仲間が逃げ始めたら（警戒の合図）、自分に天敵が見えていなくても一緒に逃げる
        wire_hidden(&mut g, h_alarm, IN_KIN_ALERT, 4.0);
        wire_out(&mut g, OUT_PRI_FLEE, h_alarm, 8.0);
        wire_out(&mut g, OUT_PRI_FOOD, h_threat, -4.0);
        wire_out(&mut g, OUT_PRI_MATE, h_threat, -6.0);
        // 足元に草があればゆっくり、なければ探しに歩き、天敵が見えたら全力で逃げる
        wire_hidden(&mut g, h_plant, IN_PLANT_HERE, 4.0);
        out_bias(&mut g, OUT_THROTTLE, 0.5);
        wire_out(&mut g, OUT_THROTTLE, h_plant, -2.5);
        wire_out(&mut g, OUT_THROTTLE, h_threat, 6.0);
    } else {
        // 獲物が見えたら、相手探しより狩りを優先し、速く追う
        let h_prey = h_threat;
        wire_hidden(&mut g, h_prey, IN_PREY_NEAR, 4.0);
        wire_out(&mut g, OUT_PRI_FOOD, h_prey, 3.0);
        out_bias(&mut g, OUT_THROTTLE, -0.5);
        wire_out(&mut g, OUT_THROTTLE, h_prey, 6.0);
    }
    // 繁殖できるときは繁殖し、食べられるときは食べる
    out_bias(&mut g, OUT_REPRODUCE, 2.0);
    out_bias(&mut g, OUT_EAT, 2.0);
    g
}

/// 群れの知覚につながる配線（中間ニューロン 6, 7）を、小さなランダムな重みで足す。
/// 手で群れを作る配線を入れると「進化で生まれた群れ」ではなくなるので、ばらつきの元だけ用意して、
/// 群れるかどうかは進化に任せる。初期の脳では中間ニューロン 6, 7 はどちらの種類も使っていない
pub fn add_kin_wiring(g: &mut [f32], rng: &mut Rng) {
    const SIGMA_IN: f32 = 1.0;
    const SIGMA_OUT: f32 = 0.3;
    let kin_inputs = [
        IN_KIN_FWD,
        IN_KIN_RIGHT,
        IN_KIN_NEAR,
        IN_KIN_COUNT,
        IN_KIN_HEAD_FWD,
        IN_KIN_HEAD_RIGHT,
        IN_KIN_ALERT,
        IN_SIM_FWD,
        IN_SIM_RIGHT,
        IN_SIM_RATIO,
    ];
    for h in [6, 7] {
        for &k in &kin_inputs {
            wire_hidden(g, h, k, rng.gaussian() * SIGMA_IN);
        }
        for o in [OUT_DIR_FWD, OUT_DIR_RIGHT, OUT_PRI_FREE, OUT_PRI_KIN, OUT_VIGILANCE, OUT_THROTTLE] {
            wire_out(g, o, h, rng.gaussian() * SIGMA_OUT);
        }
    }
}

/// 2 つの遺伝子の違い（遺伝的距離）。標識色を除く全遺伝子の差を、変異 1 回の大きさを単位にして
/// 二乗平均の平方根を取る。形質は比（対数）で、脳の重みは差で測る。
/// 同じ系統の中では小さく、別々に変異を重ねた系統どうしほど大きくなる
pub fn genetic_distance(a: &[f32], b: &[f32], trait_sigma: f32, weight_sigma: f32) -> f32 {
    let mut sum = 0.0f32;
    let mut n = 0;
    for k in 0..N_TRAITS {
        if k == G_HUE {
            continue; // 標識色は見分けの手がかりで、交配の可否には使わない
        }
        let d = (a[k].abs().max(1e-3) / b[k].abs().max(1e-3)).ln() / trait_sigma;
        sum += d * d;
        n += 1;
    }
    for k in N_TRAITS..GENOME_LEN {
        let d = (a[k] - b[k]) / weight_sigma;
        sum += d * d;
        n += 1;
    }
    (sum / n as f32).sqrt()
}

/// 遺伝的距離 distance の相手との繁殖が成功する確率。
/// full 以下なら必ず成功、zero 以上なら必ず失敗、間はなめらかに下がる（smoothstep）
pub fn compatibility(distance: f32, full: f32, zero: f32) -> f32 {
    let t = ((distance - full) / (zero - full)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// 2 つの標識色（0〜1 の色相）の差。色相環を一周するので、0.95 と 0.05 の差は 0.1
pub fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}

/// 両親の遺伝子を混ぜた子の遺伝子を out の末尾に追加する。
/// 体の形質は 1 個ずつ、脳はニューロン単位（1 つのニューロンへ入る重みとバイアスのまとまり）で、
/// どちらかの親からそのまま受け継ぐ。脳の重みを 1 個ずつ混ぜると、ニューロンへ入る重みが
/// 両親の寄せ集めになり、親が持っていた機能が壊れやすいため
pub fn crossover(a: &[f32], b: &[f32], rng: &mut Rng, out: &mut Vec<f32>) {
    let start = out.len();
    out.extend_from_slice(a);
    let child = &mut out[start..];
    for k in 0..N_TRAITS {
        if rng.f32() < 0.5 {
            child[k] = b[k];
        }
    }
    let blocks = (0..N_HIDDEN)
        .map(|h| W1 + h * (N_IN + 1)..W1 + (h + 1) * (N_IN + 1))
        .chain((0..N_OUT).map(|o| W2 + o * (N_HIDDEN + 1)..W2 + (o + 1) * (N_HIDDEN + 1)));
    for range in blocks {
        if rng.f32() < 0.5 {
            child[range.clone()].copy_from_slice(&b[range]);
        }
    }
}

/// 変異の設定
pub struct Mutation {
    /// 遺伝子 1 個あたりの変異確率
    pub rate: f32,
    /// 形質の変異の大きさ（現在値に対する割合）
    pub trait_sigma: f32,
    /// 脳の重みの変異の大きさ
    pub weight_sigma: f32,
    /// 標識色の変異の大きさ（毎回必ず漂う）
    pub hue_sigma: f32,
}

/// 遺伝子に変異を加える
pub fn mutate(g: &mut [f32], m: &Mutation, rng: &mut Rng) {
    for (k, v) in g.iter_mut().enumerate() {
        if k == G_HUE {
            *v += rng.gaussian() * m.hue_sigma;
            continue;
        }
        if rng.f32() >= m.rate {
            continue;
        }
        if k < N_TRAITS {
            *v *= 1.0 + rng.gaussian() * m.trait_sigma;
        } else {
            *v += rng.gaussian() * m.weight_sigma;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> [f32; N_IN] {
        [0.0; N_IN]
    }

    /// 欲求ごとの方向 [食べる, 繁殖, 逃げる, 仲間, (自由)]
    fn drives(food: (f32, f32), mate: (f32, f32), flee: (f32, f32)) -> [(f32, f32); N_DRIVES] {
        [food, mate, flee, (0.0, 0.0), (0.0, 0.0)]
    }

    #[test]
    fn 初期の脳は満腹なら食べるのを控える() {
        for herb in [true, false] {
            let g = initial(herb, 1.0, 1.3, 30.0, 0.3);
            let mut hungry = input();
            hungry[IN_ENERGY] = 0.5;
            let mut full = input();
            full[IN_ENERGY] = 2.0;
            let (h, f) = (think(&g, &hungry), think(&g, &full));
            assert!(f[OUT_EAT] < 0.0 && h[OUT_EAT] > 0.0, "満腹なら食べない");
            assert!(priorities(&f)[0] < priorities(&h)[0], "満腹なら食べる優先度が下がる");
        }
    }

    #[test]
    fn 初期の草食は仲間が逃げたら一緒に逃げる() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let calm = think(&g, &input());
        let mut i = input();
        i[IN_KIN_ALERT] = 0.5;
        let alarmed = think(&g, &i);
        assert!(priorities(&alarmed)[2] > 0.7 && priorities(&calm)[2] < 0.2);
    }

    #[test]
    fn 初期の草食は天敵が見えたら草より逃げるを優先する() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_NEAR] = 0.8;
        // 草は前、天敵は右（逃げる方向は左）
        let (_, r) = mix_drives(&think(&g, &i), &drives((1.0, 0.0), (0.0, 0.0), (0.0, -1.0)));
        assert!(r < -0.3, "左へ逃げるはず: {r}");
    }

    #[test]
    fn 初期の草食は正面の天敵から後ろへ逃げる() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_NEAR] = 0.8;
        let (f, _) = mix_drives(&think(&g, &i), &drives((1.0, 0.0), (0.0, 0.0), (-1.0, 0.0)));
        assert!(f < 0.0, "後ろへ逃げるはず: {f}");
    }

    #[test]
    fn 初期の脳は繁殖できる状態なら相手へ向かう() {
        for herb in [true, false] {
            let g = initial(herb, 1.0, 1.3, 30.0, 0.3);
            // 食べ物は前、相手は右
            let d = drives((1.0, 0.0), (0.0, 1.0), (0.0, 0.0));
            let mut i = input();
            i[IN_ENERGY] = 0.5;
            let (_, r_not_ready) = mix_drives(&think(&g, &i), &d);
            i[IN_ENERGY] = 1.2;
            i[IN_READY] = 1.0;
            i[IN_MATE_NEAR] = 0.8;
            let (f, r) = mix_drives(&think(&g, &i), &d);
            assert!(r > f, "繁殖できる状態では相手（右）を優先するはず: 前 {f} 右 {r}");
            assert!(r > r_not_ready, "繁殖できない状態より相手へ向かうはず");
        }
    }

    #[test]
    fn 初期の草食は天敵が近いと速くなる() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut calm = input();
        calm[IN_PLANT_HERE] = 0.5;
        let mut danger = calm;
        danger[IN_PRED_NEAR] = 0.8;
        assert!(think(&g, &danger)[OUT_THROTTLE] > think(&g, &calm)[OUT_THROTTLE]);
    }

    #[test]
    fn 初期の肉食は右の獲物へ向かう() {
        let g = initial(false, 1.3, 1.8, 48.0, 0.0);
        let mut i = input();
        i[IN_PREY_NEAR] = 0.75;
        let (f, r) = mix_drives(&think(&g, &i), &drives((0.0, 1.0), (0.0, 0.0), (0.0, 0.0)));
        assert!(r > f, "右の獲物へ向かうはず: 前 {f} 右 {r}");
    }

    #[test]
    fn 初期の脳は食べて繁殖する() {
        for herb in [true, false] {
            let out = think(&initial(herb, 1.0, 1.0, 30.0, 0.0), &input());
            assert!(out[OUT_EAT] > 0.0 && out[OUT_REPRODUCE] > 0.0);
        }
    }

    #[test]
    fn 交叉した子の遺伝子は各部分がどちらかの親のもの() {
        let a = vec![1.0f32; GENOME_LEN];
        let b = vec![2.0f32; GENOME_LEN];
        let mut out = Vec::new();
        crossover(&a, &b, &mut Rng::new(9), &mut out);
        assert_eq!(out.len(), GENOME_LEN);
        assert!(out.iter().all(|&v| v == 1.0 || v == 2.0));
        assert!(out.contains(&1.0) && out.contains(&2.0), "両親から受け継いでいる");
        // 1 つのニューロンへ入る重みは、同じ親から来る
        for h in 0..N_HIDDEN {
            let block = &out[W1 + h * (N_IN + 1)..W1 + (h + 1) * (N_IN + 1)];
            assert!(block.iter().all(|&v| v == block[0]), "中間ニューロン {h} が混ざっている");
        }
    }

    #[test]
    fn 遺伝的距離は同じなら0で変異を重ねるほど大きい() {
        let m = Mutation { rate: 0.02, trait_sigma: 0.05, weight_sigma: 0.1, hue_sigma: 0.01 };
        let base = initial(true, 1.0, 1.3, 30.0, 0.3);
        assert_eq!(genetic_distance(&base, &base, 0.05, 0.1), 0.0);
        let mut rng = Rng::new(1);
        let (mut a, mut b) = (base.clone(), base.clone());
        let mut last = 0.0;
        for generation in 1..=200 {
            mutate(&mut a, &m, &mut rng);
            mutate(&mut b, &m, &mut rng);
            if generation % 50 == 0 {
                let d = genetic_distance(&a, &b, 0.05, 0.1);
                assert!(d > last, "{generation} 世代: {d} が前回 {last} 以下");
                last = d;
            }
        }
    }

    #[test]
    fn 相性は距離に応じてなめらかに下がる() {
        assert_eq!(compatibility(0.5, 1.2, 2.5), 1.0);
        assert_eq!(compatibility(3.0, 1.2, 2.5), 0.0);
        let mid = compatibility(1.85, 1.2, 2.5);
        assert!((mid - 0.5).abs() < 1e-5);
    }

    #[test]
    fn 色相の差は一周を考慮する() {
        assert!((hue_distance(0.95, 0.05) - 0.1).abs() < 1e-6);
        assert!((hue_distance(0.2, 0.5) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn 変異は同じ乱数なら同じ結果になる() {
        let m = Mutation { rate: 0.5, trait_sigma: 0.1, weight_sigma: 0.2, hue_sigma: 0.02 };
        let mut a = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut b = a.clone();
        mutate(&mut a, &m, &mut Rng::new(5));
        mutate(&mut b, &m, &mut Rng::new(5));
        assert_eq!(a, b);
        assert_ne!(a, initial(true, 1.0, 1.3, 30.0, 0.3));
    }
}

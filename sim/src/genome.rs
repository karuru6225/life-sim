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
pub const N_TRAITS: usize = 5;

/// 形質が取りうる範囲（変異でこの外に出たら切り詰める）
pub const SIZE_RANGE: (f32, f32) = (0.4, 3.0);
pub const SPEED_RANGE: (f32, f32) = (0.2, 4.0);
pub const VISION_RANGE: (f32, f32) = (5.0, VISION_MAX);
pub const INVEST_RANGE: (f32, f32) = (0.1, 0.9);
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
pub const N_IN: usize = 13;

pub const N_HIDDEN: usize = 8;

// ---- 脳の出力 ----
// 「曲がる量」ではなく「行きたい方向」を出し、その方向へ最大旋回角まで向きを変える。
// 曲がる量だと、天敵が真正面にいるとき右方成分がほぼ 0 で曲がれず、突っ込んでしまう。
// 方向なら「天敵と反対へ」を (−前方, −右方) と素直に表せる。
/// 行きたい方向の前方成分
pub const OUT_DIR_FWD: usize = 0;
/// 行きたい方向の右方成分
pub const OUT_DIR_RIGHT: usize = 1;
/// 速さ（最高速度に対する割合に変換して使う）
pub const OUT_THROTTLE: usize = 2;
/// 正なら繁殖する（エネルギーが足りていれば）
pub const OUT_REPRODUCE: usize = 3;
/// 正なら食べる（草食は目の前の草、肉食は届く範囲の獲物）
pub const OUT_EAT: usize = 4;
pub const N_OUT: usize = 5;

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

/// 体の形質を範囲内に収めて読み出したもの
#[derive(Clone, Copy, Debug)]
pub struct Traits {
    pub size: f32,
    pub max_speed: f32,
    pub vision: f32,
    pub invest: f32,
    /// 0〜1（色相）
    pub hue: f32,
    /// size の 0.75 乗（代謝の計算用。毎 tick 計算すると重いので先に求めておく）
    pub size_pow075: f32,
    /// size の平方根（寿命と小回りの計算用）
    pub size_sqrt: f32,
    /// 1 tick に曲がれる最大の角度の cos と sin（大きいほど小回りが利かない）
    pub turn_cos: f32,
    pub turn_sin: f32,
}

/// 遺伝子から形質を読み出す。max_turn は大きさ 1 のときの最大旋回角（ラジアン）
pub fn traits(genome: &[f32], max_turn: f32) -> Traits {
    let size = genome[G_SIZE].clamp(SIZE_RANGE.0, SIZE_RANGE.1);
    let (turn_sin, turn_cos) = (max_turn / size.sqrt()).min(std::f32::consts::PI).sin_cos();
    Traits {
        size,
        max_speed: genome[G_SPEED].clamp(SPEED_RANGE.0, SPEED_RANGE.1),
        vision: genome[G_VISION].clamp(VISION_RANGE.0, VISION_RANGE.1),
        invest: genome[G_INVEST].clamp(INVEST_RANGE.0, INVEST_RANGE.1),
        hue: genome[G_HUE].rem_euclid(1.0),
        size_pow075: size.powf(0.75),
        size_sqrt: size.sqrt(),
        turn_cos,
        turn_sin,
    }
}

/// 中間層ニューロン h を、入力 k の 1 本だけにつなぐ（手で配線するとき用）
fn wire_hidden(g: &mut [f32], h: usize, input: usize, weight: f32) {
    g[W1 + h * (N_IN + 1) + input] = weight;
}

fn wire_out(g: &mut [f32], o: usize, hidden: usize, weight: f32) {
    g[W2 + o * (N_HIDDEN + 1) + hidden] = weight;
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

    // 何もなければ、ゆるく前へ進む
    out_bias(&mut g, OUT_DIR_FWD, 0.3);

    if herbivore {
        let (h_grad_f, h_grad_r, h_pred_f, h_pred_r, h_pred_near, h_plant) = (0, 1, 2, 3, 4, 5);
        wire_hidden(&mut g, h_grad_f, IN_GRAD_FWD, 4.0);
        wire_hidden(&mut g, h_grad_r, IN_GRAD_RIGHT, 4.0);
        wire_hidden(&mut g, h_pred_f, IN_PRED_FWD, 4.0);
        wire_hidden(&mut g, h_pred_r, IN_PRED_RIGHT, 4.0);
        wire_hidden(&mut g, h_pred_near, IN_PRED_NEAR, 4.0);
        wire_hidden(&mut g, h_plant, IN_PLANT_HERE, 4.0);
        // 草の多い方へ向かい、天敵が見えたらそれより強く反対方向へ向かう
        wire_out(&mut g, OUT_DIR_FWD, h_grad_f, 2.0);
        wire_out(&mut g, OUT_DIR_RIGHT, h_grad_r, 2.0);
        wire_out(&mut g, OUT_DIR_FWD, h_pred_f, -6.0);
        wire_out(&mut g, OUT_DIR_RIGHT, h_pred_r, -6.0);
        // 足元に草があればゆっくり、なければ探しに歩き、天敵が見えたら全力で逃げる
        out_bias(&mut g, OUT_THROTTLE, 0.5);
        wire_out(&mut g, OUT_THROTTLE, h_plant, -2.5);
        wire_out(&mut g, OUT_THROTTLE, h_pred_near, 6.0);
    } else {
        let (h_prey_f, h_prey_r, h_prey_near) = (0, 1, 2);
        wire_hidden(&mut g, h_prey_f, IN_PREY_FWD, 4.0);
        wire_hidden(&mut g, h_prey_r, IN_PREY_RIGHT, 4.0);
        wire_hidden(&mut g, h_prey_near, IN_PREY_NEAR, 4.0);
        // 獲物の方へ向かい、獲物が見えたら速く追う
        wire_out(&mut g, OUT_DIR_FWD, h_prey_f, 6.0);
        wire_out(&mut g, OUT_DIR_RIGHT, h_prey_r, 6.0);
        out_bias(&mut g, OUT_THROTTLE, -0.5);
        wire_out(&mut g, OUT_THROTTLE, h_prey_near, 6.0);
    }
    // 繁殖できるときは繁殖し、食べられるときは食べる
    out_bias(&mut g, OUT_REPRODUCE, 2.0);
    out_bias(&mut g, OUT_EAT, 2.0);
    g
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

    #[test]
    fn 初期の草食は右の天敵から左へ逃げる() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_RIGHT] = 1.0;
        i[IN_PRED_NEAR] = 0.8;
        assert!(think(&g, &i)[OUT_DIR_RIGHT] < 0.0);
    }

    #[test]
    fn 初期の草食は正面の天敵から後ろへ逃げる() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_FWD] = 1.0;
        i[IN_PRED_NEAR] = 0.8;
        assert!(think(&g, &i)[OUT_DIR_FWD] < 0.0);
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
        i[IN_PREY_RIGHT] = 1.0;
        i[IN_PREY_NEAR] = 0.5;
        assert!(think(&g, &i)[OUT_DIR_RIGHT] > 0.0);
    }

    #[test]
    fn 初期の脳は食べて繁殖する() {
        for herb in [true, false] {
            let out = think(&initial(herb, 1.0, 1.0, 30.0, 0.0), &input());
            assert!(out[OUT_EAT] > 0.0 && out[OUT_REPRODUCE] > 0.0);
        }
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

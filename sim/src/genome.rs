//! 遺伝子と脳。
//!
//! 1 個体の遺伝子は f32 の並び（長さ GENOME_LEN）で、前半が体の形質、後半が脳の重み。
//! 全個体の遺伝子は 1 本の配列に GENOME_LEN ずつ詰めて持つ（SoA の一種）。
//!
//! 脳は 入力 N_IN → 中間 1 層目（最大 H1）→ 中間 2 層目（最大 H2）→ 出力 N_OUT のニューラルネット。
//! 出力は 1 層目と 2 層目の両方から受け取る（1 層目からの近道があるので、2 層目を使わなくても動く）。
//! 中間ニューロンはそれぞれ「使う／使わない」の遺伝子を持ち、使うニューロンの数だけ維持費がかかる。
//! 使わないニューロンは計算せず、重みも変異しない（使い始めたときに重み 0 から育つ）。
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
/// 好みの気温（度）。今いる場所の気温との差に応じて、体温を保つ代謝が増える。
/// 0 や負にもなるので、他の形質のような「何倍」ではなく「± 何度」で変異させる
pub const G_TEMP_PREF: usize = 7;
/// 視野角（ラジアン）。正面を中心にこの角度の扇形の中だけが見える。2π なら全方位
pub const G_FOV: usize = 8;
pub const N_TRAITS: usize = 9;

/// 形質が取りうる範囲（変異でこの外に出たら切り詰める）
pub const SIZE_RANGE: (f32, f32) = (0.4, 3.0);
pub const SPEED_RANGE: (f32, f32) = (0.2, 4.0);
pub const VISION_RANGE: (f32, f32) = (5.0, VISION_MAX);
pub const INVEST_RANGE: (f32, f32) = (0.1, 0.9);
pub const PREY_MIN_RANGE: (f32, f32) = (0.05, 2.0);
pub const PREY_MAX_RANGE: (f32, f32) = (0.2, 4.0);
pub const TEMP_PREF_RANGE: (f32, f32) = (-30.0, 50.0);
pub const FOV_RANGE: (f32, f32) = (0.5, std::f32::consts::TAU);
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
/// 今いる場所の気温 − 好みの気温（10 度を 1 とする）
pub const IN_TEMP_DIFF: usize = 26;
/// 季節（−1〜1。日射が最も多い時期に 1、最も少ない時期に −1。季節がない設定なら 0）
pub const IN_SEASON: usize = 27;
/// 見えている仲間が出している繁殖シグナルの平均（0〜1）
pub const IN_KIN_SIGNAL: usize = 28;
/// 同じ種類の繁殖シグナル（呼び声）が大きく聞こえる方向（自分基準の前方・右方成分）。視野の外まで届く
pub const IN_CALL_FWD: usize = 29;
pub const IN_CALL_RIGHT: usize = 30;
/// 周りの呼び声の大きさ（0〜1）
pub const IN_CALL_LEVEL: usize = 31;
/// 死角の見張り度: 見えている仲間のうち、自分の真後ろを視野に収めている個体の割合（0〜1）。
/// 仲間が背後を見てくれていれば、自分は警戒を緩めて食べることに集中できる（多くの目の効果）
pub const IN_REAR_WATCH: usize = 32;
/// 足元の死体の量（softsign で 0〜1 に縮める。草食は常に 0）
pub const IN_CARRION_HERE: usize = 33;
/// 周りで死体の多い方向（自分基準の前方・右方成分。草食は常に 0）
pub const IN_CARRION_FWD: usize = 34;
pub const IN_CARRION_RIGHT: usize = 35;
/// 足元の死体の新しさ（1 で死んだばかり、古いほど 0 に近づく。死体がなければ 0）。古い死体は栄養が少なく、病気になりやすい
pub const IN_CARRION_FRESH: usize = 36;
// 同じ種類の敵（系統色が自分と離れている個体）
/// 最寄りの敵の方向（自分基準の前方・右方成分）
pub const IN_RIVAL_FWD: usize = 37;
pub const IN_RIVAL_RIGHT: usize = 38;
/// 最寄りの敵が見えていれば 0.5〜1（近いほど大きい）、見えていなければ 0
pub const IN_RIVAL_NEAR: usize = 39;
/// 最寄りの敵の大きさ ÷ 自分の大きさ（見えていなければ 0）
pub const IN_RIVAL_SIZE: usize = 40;
/// 見えている敵の数（softsign(数 / 4) で 0〜1 に縮める）
pub const IN_RIVAL_COUNT: usize = 41;
/// 最近、敵に攻撃されて負けたか（負けた直後に 1、ATTACKED_TICKS かけて 0 に戻る）
pub const IN_ATTACKED: usize = 42;
/// 仲間（同じ種類）を最後に見てからの時間（softsign(tick 数 ÷ 150)。仲間が見えていれば 0、長く一人でいるほど 1 に近い）
pub const IN_ALONE: usize = 43;
pub const N_IN: usize = 44;
/// 「前方が水か」を調べる距離（px）
pub const WATER_LOOKAHEAD: f32 = 12.0;

/// 中間 1 層目と 2 層目のニューロンの最大数
pub const H1: usize = 16;
pub const H2: usize = 16;
pub const N_HIDDEN: usize = H1 + H2;
/// 初期の脳で使う 1 層目のニューロンの数（0〜7 番）。2 層目は使わない
pub const INITIAL_H1: usize = 8;

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
/// 繁殖シグナルの強さ（0〜1 に変換して使う）。周りの仲間に見え、出すほどエネルギーを使い、草に隠れにくくなる
pub const OUT_SIGNAL: usize = 11;
/// 敵を追い払う欲求の優先度（最寄りの敵へ）
pub const OUT_PRI_FIGHT: usize = 12;
/// 正なら、届く距離の敵を攻撃する
pub const OUT_ATTACK: usize = 13;
pub const N_OUT: usize = 14;

/// 欲求の数（食べる・繁殖・逃げる・仲間・追い払う・自由）。最後の「自由」の方向は脳の出力から取る
pub const N_DRIVES: usize = 6;

/// 出力から欲求ごとの優先度（0〜1）を取り出す
pub fn priorities(out: &[f32; N_OUT]) -> [f32; N_DRIVES] {
    [OUT_PRI_FOOD, OUT_PRI_MATE, OUT_PRI_FLEE, OUT_PRI_KIN, OUT_PRI_FIGHT, OUT_PRI_FREE].map(|k| 0.5 + 0.5 * softsign(out[k]))
}

/// 出力から警戒の度合い（0〜1）を取り出す
pub fn vigilance(out: &[f32; N_OUT]) -> f32 {
    0.5 + 0.5 * softsign(out[OUT_VIGILANCE])
}

/// 出力から繁殖シグナルの強さ（0〜1）を取り出す
pub fn signal(out: &[f32; N_OUT]) -> f32 {
    0.5 + 0.5 * softsign(out[OUT_SIGNAL])
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

// 遺伝子の並び: [形質 N_TRAITS][使うかどうか N_HIDDEN][1 層目の重み][2 層目の重み][出力の重み]
/// 中間ニューロンを使うかどうか（正なら使う）。1 層目が 0..H1、2 層目が H1..N_HIDDEN
const ACT: usize = N_TRAITS;
/// 1 層目: 1 ニューロンあたり N_IN 個の重み + バイアス
const W1: usize = ACT + N_HIDDEN;
const W1_ROW: usize = N_IN + 1;
/// 2 層目: 1 ニューロンあたり H1 個の重み + バイアス
const W2: usize = W1 + H1 * W1_ROW;
const W2_ROW: usize = H1 + 1;
/// 出力: 1 出力あたり、1 層目と 2 層目の N_HIDDEN 個の重み + バイアス
const W3: usize = W2 + H2 * W2_ROW;
const W3_ROW: usize = N_HIDDEN + 1;
pub const GENOME_LEN: usize = W3 + N_OUT * W3_ROW;

/// 中間ニューロン h（0..N_HIDDEN、H1 以上は 2 層目）を使っているか
pub fn neuron_active(genome: &[f32], h: usize) -> bool {
    genome[ACT + h] > 0.0
}

/// 使っている中間ニューロンの数（1 層目, 2 層目）
pub fn active_neurons(genome: &[f32]) -> (u32, u32) {
    let count = |r: std::ops::Range<usize>| r.filter(|&h| neuron_active(genome, h)).count() as u32;
    (count(0..H1), count(H1..N_HIDDEN))
}

pub fn softsign(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

/// 脳を 1 回動かす。使っている中間ニューロンだけを計算し、その重みだけを読む
pub fn think(genome: &[f32], input: &[f32; N_IN]) -> [f32; N_OUT] {
    // 使っているニューロンの (番号, 出力)。2 層目と出力層は、これだけを読む
    let mut idx = [0usize; N_HIDDEN];
    let mut val = [0.0f32; N_HIDDEN];
    let mut n = 0;
    for h in 0..H1 {
        if genome[ACT + h] <= 0.0 {
            continue;
        }
        let w = &genome[W1 + h * W1_ROW..W1 + (h + 1) * W1_ROW];
        let mut sum = w[N_IN]; // バイアス
        for k in 0..N_IN {
            sum += w[k] * input[k];
        }
        idx[n] = h;
        val[n] = softsign(sum);
        n += 1;
    }
    let n1 = n;
    for h in 0..H2 {
        if genome[ACT + H1 + h] <= 0.0 {
            continue;
        }
        let w = &genome[W2 + h * W2_ROW..W2 + (h + 1) * W2_ROW];
        let mut sum = w[H1];
        for k in 0..n1 {
            sum += w[idx[k]] * val[k];
        }
        idx[n] = H1 + h;
        val[n] = softsign(sum);
        n += 1;
    }
    let mut output = [0.0f32; N_OUT];
    for (o, out) in output.iter_mut().enumerate() {
        let w = &genome[W3 + o * W3_ROW..W3 + (o + 1) * W3_ROW];
        let mut sum = w[N_HIDDEN];
        for k in 0..n {
            sum += w[idx[k]] * val[k];
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
    /// 好みの気温（度）
    pub temp_pref: f32,
    /// 体温を保つ費用の大きさによる倍率（いまの大きさの 0.42 乗）。
    /// 代謝は大きさの 0.75 乗、熱の逃げやすさは体積に対する表面積で大きさの −1/3 乗。大きいほど体積あたりは割安
    pub thermal_scale: f32,
    /// 0〜1（色相）
    pub hue: f32,
    /// 視野角（ラジアン）と、その半分の cos（見えるかの判定用）
    pub fov: f32,
    pub fov_cos: f32,
    /// 使っている中間ニューロンの数（1 層目, 2 層目）。脳の維持費に使う
    pub brain: (u32, u32),
}

impl Traits {
    /// 成長度 growth（0〜1）に合わせて、いまの大きさと、それから決まる値を計算し直す
    pub fn grow_to(&mut self, growth: f32, max_turn: f32) {
        self.size = self.adult_size * growth;
        self.size_pow075 = self.size.powf(0.75);
        self.thermal_scale = self.size.powf(0.75 - 1.0 / 3.0);
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
        temp_pref: genome[G_TEMP_PREF].clamp(TEMP_PREF_RANGE.0, TEMP_PREF_RANGE.1),
        thermal_scale: 0.0,
        hue: genome[G_HUE].rem_euclid(1.0),
        fov: 0.0,
        fov_cos: 0.0,
        brain: active_neurons(genome),
    };
    t.fov = genome[G_FOV].clamp(FOV_RANGE.0, FOV_RANGE.1);
    t.fov_cos = (t.fov * 0.5).cos();
    t.grow_to(1.0, max_turn);
    t
}

/// 中間層ニューロン h を、入力 k の 1 本だけにつなぐ（手で配線するとき用）
/// 1 層目のニューロン h を、入力 k につなぐ（手で配線するとき用）
fn wire_hidden(g: &mut [f32], h: usize, input: usize, weight: f32) {
    g[W1 + h * W1_ROW + input] = weight;
}

/// 1 層目のニューロン hidden を、出力 o につなぐ
fn wire_out(g: &mut [f32], o: usize, hidden: usize, weight: f32) {
    g[W3 + o * W3_ROW + hidden] = weight;
}

fn hidden_bias(g: &mut [f32], h: usize, bias: f32) {
    g[W1 + h * W1_ROW + N_IN] = bias;
}

fn out_bias(g: &mut [f32], o: usize, bias: f32) {
    g[W3 + o * W3_ROW + N_HIDDEN] = bias;
}

/// 初期個体の遺伝子。脳は Phase 1 の手書きルールとほぼ同じ動きをするように配線してある。
/// 進化はここから始まる。
/// 中間層の各ニューロンは入力 1 本を受け取るだけ（ほぼ素通し）で、割り当ては種類ごとに違う
pub fn initial(herbivore: bool, size: f32, max_speed: f32, vision: f32, hue: f32) -> Vec<f32> {
    let mut g = vec![0.0; GENOME_LEN];
    // 1 層目の 0〜7 番だけを使う。残りと 2 層目は使わない（進化で使い始めるかもしれない）
    for h in 0..N_HIDDEN {
        g[ACT + h] = if h < INITIAL_H1 { 1.0 } else { -1.0 };
    }
    g[G_SIZE] = size;
    g[G_SPEED] = max_speed;
    g[G_VISION] = vision;
    g[G_INVEST] = 0.5;
    g[G_HUE] = hue;
    // 自分の 0.4〜1.3 倍の獲物を狙う（小さすぎると割に合わず、大きすぎると狩れないうえ反撃を受ける）
    g[G_PREY_MIN] = 0.4;
    g[G_PREY_MAX] = 1.3;
    // 好みの気温は、初期個体を置くときにその場所の気温で上書きする
    g[G_TEMP_PREF] = 15.0;
    // 視野角: 草食は目が横に付いて広く（死角は真後ろだけ）、肉食は目が前に付いて狭い
    g[G_FOV] = if herbivore { 300f32.to_radians() } else { 120f32.to_radians() };

    // 自由な移動は、ゆるく前へ
    out_bias(&mut g, OUT_DIR_FWD, 0.3);

    // 優先度の基本: 普段は食べる、繁殖できる状態なら繁殖、天敵が見えたら逃げるを最優先。
    // 仲間に寄る欲求は弱くしておき、強めるかどうかは進化に任せる
    // 中間ニューロン 5 は繁殖期の気分（下）、6, 7 は群れの知覚への配線（add_kin_wiring）
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
    // 繁殖期: 日射の多い季節と、周りの仲間が繁殖シグナルを出しているときは、繁殖の欲求が強まる
    let h_mood = 5;
    wire_hidden(&mut g, h_mood, IN_SEASON, 2.0);
    wire_hidden(&mut g, h_mood, IN_KIN_SIGNAL, 3.0);
    wire_out(&mut g, OUT_PRI_MATE, h_mood, 3.0);
    // 繁殖シグナルは、繁殖できる状態で、繁殖期の気分が高まっているときに出す
    out_bias(&mut g, OUT_SIGNAL, -3.0);
    wire_out(&mut g, OUT_SIGNAL, h_ready, 3.0);
    wire_out(&mut g, OUT_SIGNAL, h_mood, 2.0);
    // 敵を追い払う欲求と攻撃は、初めはほとんど使わない（使うかどうかは進化に任せる）
    out_bias(&mut g, OUT_PRI_FIGHT, -4.0);
    out_bias(&mut g, OUT_ATTACK, -2.0);
    // 敵に攻撃されて負けたら逃げる（草食は警戒の合図のニューロン、肉食は空いている 4 番を使う）
    wire_hidden(&mut g, h_alarm, IN_ATTACKED, 4.0);
    if !herbivore {
        wire_out(&mut g, OUT_PRI_FLEE, h_alarm, 8.0);
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
        IN_REAR_WATCH,
        IN_RIVAL_FWD,
        IN_RIVAL_RIGHT,
        IN_RIVAL_NEAR,
        IN_RIVAL_SIZE,
        IN_RIVAL_COUNT,
        IN_ALONE,
    ];
    for h in [6, 7] {
        for &k in &kin_inputs {
            wire_hidden(g, h, k, rng.gaussian() * SIGMA_IN);
        }
        for o in [OUT_DIR_FWD, OUT_DIR_RIGHT, OUT_PRI_FREE, OUT_PRI_KIN, OUT_VIGILANCE, OUT_THROTTLE, OUT_PRI_FIGHT, OUT_ATTACK] {
            wire_out(g, o, h, rng.gaussian() * SIGMA_OUT);
        }
    }
}

/// 2 つの遺伝子の違い（遺伝的距離）。標識色を除く形質と、脳の重みの差を、変異 1 回の大きさを単位にして
/// 二乗平均の平方根を取る。形質は比（対数）で、脳の重みは差で測る。
/// 脳は、どちらかが使っているニューロンだけを比べる（使っていないニューロンの重みは全員 0 なので、数えると距離が薄まる）。
/// 片方だけが使っているニューロンは、大きな違い（変異 NEURON_MISMATCH 回分）1 つとして数える。
/// 同じ系統の中では小さく、別々に変異を重ねた系統どうしほど大きくなる
pub fn genetic_distance(a: &[f32], b: &[f32], trait_sigma: f32, weight_sigma: f32, temp_sigma: f32) -> f32 {
    const NEURON_MISMATCH: f32 = 3.0;
    let mut sum = 0.0f32;
    let mut n = 0;
    for k in 0..N_TRAITS {
        if k == G_HUE {
            continue; // 標識色は見分けの手がかりで、交配の可否には使わない
        }
        let d = if k == G_TEMP_PREF {
            (a[k] - b[k]) / temp_sigma
        } else {
            (a[k].abs().max(1e-3) / b[k].abs().max(1e-3)).ln() / trait_sigma
        };
        sum += d * d;
        n += 1;
    }
    let mut add = |x: f32, y: f32| {
        let d = (x - y) / weight_sigma;
        sum += d * d;
        n += 1;
    };
    for h in 0..N_HIDDEN {
        let (ia, ib) = (neuron_active(a, h), neuron_active(b, h));
        if !ia && !ib {
            continue;
        }
        if ia != ib {
            add(0.0, NEURON_MISMATCH * weight_sigma);
            continue;
        }
        // ニューロンへ入る重み（2 層目は、使っている 1 層目からの分だけ）と、出力への重み
        let row = if h < H1 { W1 + h * W1_ROW..W1 + (h + 1) * W1_ROW } else { W2 + (h - H1) * W2_ROW..W2 + (h - H1 + 1) * W2_ROW };
        for k in row.clone() {
            let col = k - row.start;
            if h >= H1 && col < H1 && !(neuron_active(a, col) || neuron_active(b, col)) {
                continue;
            }
            add(a[k], b[k]);
        }
        for o in 0..N_OUT {
            let k = W3 + o * W3_ROW + h;
            add(a[k], b[k]);
        }
    }
    for o in 0..N_OUT {
        let k = W3 + o * W3_ROW + N_HIDDEN;
        add(a[k], b[k]);
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
/// 体の形質は 1 個ずつ、脳はニューロン単位（使うかどうかと、そのニューロンへ入る重みとバイアスのまとまり、
/// 出力ニューロンへ入る重みのまとまり）で、どちらかの親からそのまま受け継ぐ。脳の重みを 1 個ずつ混ぜると、
/// ニューロンへ入る重みが両親の寄せ集めになり、親が持っていた機能が壊れやすいため
pub fn crossover(a: &[f32], b: &[f32], rng: &mut Rng, out: &mut Vec<f32>) {
    let start = out.len();
    out.extend_from_slice(a);
    let child = &mut out[start..];
    for k in 0..N_TRAITS {
        if rng.f32() < 0.5 {
            child[k] = b[k];
        }
    }
    for h in 0..N_HIDDEN {
        if rng.f32() < 0.5 {
            child[ACT + h] = b[ACT + h];
            let row = if h < H1 { W1 + h * W1_ROW..W1 + (h + 1) * W1_ROW } else { W2 + (h - H1) * W2_ROW..W2 + (h - H1 + 1) * W2_ROW };
            child[row.clone()].copy_from_slice(&b[row]);
        }
    }
    for o in 0..N_OUT {
        if rng.f32() < 0.5 {
            let row = W3 + o * W3_ROW..W3 + (o + 1) * W3_ROW;
            child[row.clone()].copy_from_slice(&b[row]);
        }
    }
}

/// 変異の設定
pub struct Mutation {
    /// 遺伝子 1 個あたりの変異確率（脳の重みは、使っているニューロンのものだけが変異する）
    pub rate: f32,
    /// 形質の変異の大きさ（現在値に対する割合）
    pub trait_sigma: f32,
    /// 脳の重みの変異の大きさ
    pub weight_sigma: f32,
    /// 標識色の変異の大きさ（毎回必ず漂う）
    pub hue_sigma: f32,
    /// 好みの気温の変異の大きさ（度、変異確率は他の遺伝子と同じ）
    pub temp_sigma: f32,
    /// 中間ニューロン 1 個あたり、使う／使わないが切り替わる確率
    pub neuron_flip: f32,
}

/// 遺伝子に変異を加える
pub fn mutate(g: &mut [f32], m: &Mutation, rng: &mut Rng) {
    for k in 0..N_TRAITS {
        if k == G_HUE {
            g[k] += rng.gaussian() * m.hue_sigma;
            continue;
        }
        if rng.f32() >= m.rate {
            continue;
        }
        if k == G_TEMP_PREF {
            g[k] += rng.gaussian() * m.temp_sigma;
        } else {
            g[k] *= 1.0 + rng.gaussian() * m.trait_sigma;
        }
    }
    // 中間ニューロンを使い始める／やめる
    for h in 0..N_HIDDEN {
        if rng.f32() < m.neuron_flip {
            g[ACT + h] = if g[ACT + h] > 0.0 { -1.0 } else { 1.0 };
        }
    }
    // 脳の重みは、使っているニューロンにつながるものだけが変異する。使っていないニューロンの重みは 0 のまま残り、
    // 使い始めたときに何もしない状態から育つ
    let mut weight = |g: &mut [f32], k: usize| {
        if rng.f32() < m.rate {
            g[k] += rng.gaussian() * m.weight_sigma;
        }
    };
    for h in 0..H1 {
        if neuron_active(g, h) {
            for k in W1 + h * W1_ROW..W1 + (h + 1) * W1_ROW {
                weight(g, k);
            }
        }
    }
    for h in 0..H2 {
        if !neuron_active(g, H1 + h) {
            continue;
        }
        let row = W2 + h * W2_ROW;
        for col in 0..W2_ROW {
            // 使っていない 1 層目からの重みは変異しない（バイアスは常に）
            if col < H1 && !neuron_active(g, col) {
                continue;
            }
            weight(g, row + col);
        }
    }
    for o in 0..N_OUT {
        let row = W3 + o * W3_ROW;
        for col in 0..W3_ROW {
            if col < N_HIDDEN && !neuron_active(g, col) {
                continue;
            }
            weight(g, row + col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> [f32; N_IN] {
        [0.0; N_IN]
    }

    /// 欲求ごとの方向 [食べる, 繁殖, 逃げる, 仲間, 追い払う, (自由)]
    fn drives(food: (f32, f32), mate: (f32, f32), flee: (f32, f32)) -> [(f32, f32); N_DRIVES] {
        [food, mate, flee, (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]
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
    fn 初期の脳は繁殖期と周りのシグナルで繁殖の欲求が強まる() {
        for herb in [true, false] {
            let g = initial(herb, 1.0, 1.3, 30.0, 0.3);
            let mut i = input();
            i[IN_ENERGY] = 1.2;
            i[IN_READY] = 1.0;
            let mate = |i: &[f32; N_IN]| priorities(&think(&g, i))[1];
            i[IN_SEASON] = -1.0;
            let off = mate(&i);
            i[IN_SEASON] = 1.0;
            let on = mate(&i);
            i[IN_KIN_SIGNAL] = 0.8;
            let crowd = mate(&i);
            assert!(on > off + 0.1, "繁殖期のほうが強い: 期外 {off} 繁殖期 {on}");
            assert!(crowd > on, "周りがシグナルを出しているともっと強い: {on} → {crowd}");
        }
    }

    #[test]
    fn 初期の脳は繁殖できる状態の繁殖期にシグナルを出す() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_ENERGY] = 0.5;
        i[IN_SEASON] = -1.0;
        let quiet = signal(&think(&g, &i));
        i[IN_ENERGY] = 1.2;
        i[IN_READY] = 1.0;
        i[IN_SEASON] = 1.0;
        let loud = signal(&think(&g, &i));
        assert!(quiet < 0.15 && loud > 0.5, "期外 {quiet} 繁殖期 {loud}");
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
        // 1 つのニューロンへ入る重みと、使うかどうかは、同じ親から来る
        for h in 0..H1 {
            let block = &out[W1 + h * W1_ROW..W1 + (h + 1) * W1_ROW];
            assert!(block.iter().all(|&v| v == block[0]), "中間ニューロン {h} が混ざっている");
            assert_eq!(out[ACT + h], block[0]);
        }
    }

    #[test]
    fn 遺伝的距離は同じなら0で変異を重ねるほど大きい() {
        let m = Mutation { rate: 0.02, trait_sigma: 0.05, weight_sigma: 0.1, hue_sigma: 0.01, temp_sigma: 0.5, neuron_flip: 0.0 };
        let base = initial(true, 1.0, 1.3, 30.0, 0.3);
        assert_eq!(genetic_distance(&base, &base, 0.05, 0.1, 0.5), 0.0);
        let mut rng = Rng::new(1);
        let (mut a, mut b) = (base.clone(), base.clone());
        let mut last = 0.0;
        for generation in 1..=200 {
            mutate(&mut a, &m, &mut rng);
            mutate(&mut b, &m, &mut rng);
            if generation % 50 == 0 {
                let d = genetic_distance(&a, &b, 0.05, 0.1, 0.5);
                assert!(d > last, "{generation} 世代: {d} が前回 {last} 以下");
                last = d;
            }
        }
    }

    #[test]
    fn 初期の脳は_1_層目の_8_個だけを使う() {
        let g = initial(true, 1.0, 1.3, 30.0, 0.3);
        assert_eq!(active_neurons(&g), (INITIAL_H1 as u32, 0));
    }

    #[test]
    fn 使い始めたニューロンは重み_0_から始まり行動を変えない() {
        let mut g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_NEAR] = 0.8;
        i[IN_ENERGY] = 0.7;
        let before = think(&g, &i);
        // 1 層目の 12 番と 2 層目の 3 番を使い始める
        g[ACT + 12] = 1.0;
        g[ACT + H1 + 3] = 1.0;
        assert_eq!(active_neurons(&g), (INITIAL_H1 as u32 + 1, 1));
        assert_eq!(think(&g, &i), before);
    }

    #[test]
    fn 二層目は一層目の出力を受けて出力に効く() {
        let mut g = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut i = input();
        i[IN_PRED_NEAR] = 0.8;
        let before = think(&g, &i)[OUT_SIGNAL];
        // 2 層目の 0 番: 1 層目の 0 番（天敵）を受けて、シグナルの出力へ
        g[ACT + H1] = 1.0;
        g[W2] = 3.0;
        g[W3 + OUT_SIGNAL * W3_ROW + H1] = 2.0;
        assert!(think(&g, &i)[OUT_SIGNAL] > before + 0.5);
    }

    #[test]
    fn 使っていないニューロンの重みは変異しない() {
        let m = Mutation { rate: 1.0, trait_sigma: 0.05, weight_sigma: 0.1, hue_sigma: 0.01, temp_sigma: 0.5, neuron_flip: 0.0 };
        let mut g = initial(true, 1.0, 1.3, 30.0, 0.3);
        mutate(&mut g, &m, &mut Rng::new(3));
        let unused = INITIAL_H1 + 2;
        assert!(g[W1 + unused * W1_ROW..W1 + (unused + 1) * W1_ROW].iter().all(|&v| v == 0.0));
        assert!((0..N_OUT).all(|o| g[W3 + o * W3_ROW + unused] == 0.0));
        assert!(g[W1..W1 + W1_ROW].iter().any(|&v| v != 0.0), "使っているニューロンの重みは変異する");
    }

    #[test]
    fn 片方だけが使うニューロンは遺伝的距離を広げる() {
        let a = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut b = a.clone();
        b[ACT + 10] = 1.0;
        let d = genetic_distance(&a, &b, 0.05, 0.1, 0.5);
        assert!(d > 0.0);
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
        let m = Mutation { rate: 0.5, trait_sigma: 0.1, weight_sigma: 0.2, hue_sigma: 0.02, temp_sigma: 0.5, neuron_flip: 0.1 };
        let mut a = initial(true, 1.0, 1.3, 30.0, 0.3);
        let mut b = a.clone();
        mutate(&mut a, &m, &mut Rng::new(5));
        mutate(&mut b, &m, &mut Rng::new(5));
        assert_eq!(a, b);
        assert_ne!(a, initial(true, 1.0, 1.3, 30.0, 0.3));
    }
}

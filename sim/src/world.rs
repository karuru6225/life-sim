//! 世界と個体データ。
//!
//! 個体は「1 個体 = 1 構造体」ではなく、属性ごとの配列（SoA）で持つ。
//! i 番目の個体の位置は (xs[i], ys[i])、種類は kind[i]、
//! 遺伝子は genomes[i * GENOME_LEN .. (i + 1) * GENOME_LEN]。
//! 配列上の位置 i は死亡のたびに詰め直されて変わるので、個体を追いかけるときは id を使う。
//!
//! 個体は毎 tick 2 段で動く。
//! 1 段目（plan）: 全個体が tick の始めの世界を見て、知覚と脳の判断をする。世界に書き込まないので並列に計算できる
//!   （parallel 機能で rayon を使う。ブラウザでは順番に計算する。どちらでも結果は同じ）。
//! 2 段目（act）: 判断に従って、1 体ずつ順番に行動する（移動・食べる・狩る・争う・繁殖）。
//!
//! エネルギーは「日射 → 植物 → 草食 → 肉食」と一方通行で流れ、どこからも湧かない。
//! 出入りはすべて Ledger（帳簿）に記録し、テストで帳尻が合うことを確かめる。

use crate::genome::{self, GENOME_LEN, Mutation, N_IN, Traits, softsign};
use crate::grid::Grid;
use crate::params::Params;
use crate::plants::{LAYERS, PlantField};
use crate::rng::Rng;
use crate::terrain::{RainSpec, Terrain, TerrainSpec};
use std::collections::VecDeque;

pub mod history;
#[cfg(feature = "profile")]
pub mod profile;
mod save;
pub use history::History;
pub use save::FORMAT_VERSION as SAVE_FORMAT_VERSION;
pub use save::{load_file, save_file};

pub const HERBIVORE: u8 = 0;
pub const CARNIVORE: u8 = 1;

/// いまの状況（観察用）
pub mod state {
    /// 特に何もない（肉食が獲物を探している）
    pub const WANDER: u8 = 0;
    /// 草を食べた
    pub const GRAZE: u8 = 1;
    /// 食べずに移動している
    pub const SEEK: u8 = 2;
    /// 天敵が見えている
    pub const FLEE: u8 = 3;
    /// 獲物が見えている
    pub const CHASE: u8 = 4;
    /// 獲物を食べた
    pub const EAT: u8 = 5;
    /// 獲物を襲ったが逃げられた
    pub const MISSED: u8 = 6;
    /// 繁殖相手へ向かっている
    pub const COURT: u8 = 7;
    /// 死体を食べた
    pub const SCAVENGE: u8 = 8;
    /// 同じ種類の敵を攻撃した
    pub const FIGHT: u8 = 9;
    /// 仲間（同じ系統）を攻撃した
    pub const FIGHT_KIN: u8 = 10;
    /// 巣を作った
    pub const BUILD: u8 = 11;
    /// 子に与えた
    pub const FEED: u8 = 12;
}

/// 死因（観察用）
pub mod death {
    pub const STARVED: u8 = 1;
    pub const OLD_AGE: u8 = 2;
    pub const EATEN: u8 = 3;
    /// 狩りで大きな獲物を襲い、反撃を受けて死んだ
    pub const INJURED: u8 = 4;
    /// 腐った死体を食べて病気になって死んだ
    pub const SICK: u8 = 5;
    /// 同じ種類の敵との争いで死んだ
    pub const FOUGHT: u8 = 6;
    /// 仲間（同じ系統）との争いで死んだ
    pub const KIN_FOUGHT: u8 = 7;
}

/// 目の維持費の換算の基準（視野角 300°・視野 30px）。この目の維持費が vision_cost × 30 になる
const EYE_REF_RANGE: f32 = 30.0;
const EYE_REF_AREA: f32 = 0.5 * (300.0 * std::f32::consts::PI / 180.0) * EYE_REF_RANGE * EYE_REF_RANGE;

/// 敵に攻撃されて負けたあと、「攻撃された」状態が続く tick 数
const ATTACKED_TICKS: u8 = 30;

/// これより少なくなった死体は消す
const CARRION_MIN: f32 = 0.1;

/// これより出来の悪い巣は崩れて消える
const NEST_MIN: f32 = 0.01;

/// 妊娠していない（または無性生殖の）ときの相手の遺伝子の置き場所
pub const NO_SLOT: u32 = u32::MAX;
/// 巣を覚えていないときの home の値
/// 体力がこれを切ると、出せる速さの上限が下がり始める
const STAMINA_FADE: f32 = 0.2;
pub const NO_HOME: u32 = u32::MAX;

/// 気温と植物の育ちやすさを更新する間隔（tick）
const CLIMATE_EVERY: u64 = 30;

/// 近傍探索のグリッドのセルの大きさ。細かいほど、視野の小さい個体が調べる範囲を絞れる
const GRID_CELL: f32 = 16.0;

/// 系統の統計（World::lineage_stats）の要素数と並び:
/// 0 個体数, 1 幼体の割合, 2 大人の大きさ, 3 最高速度, 4 視野, 5 視野角（度）, 6 好みの気温, 7 子への投資,
/// 8 繁殖シグナル, 9 世代, 10・11 重心の x・y, 12 重心からの距離の平均, 13 エネルギー ÷ 繁殖に必要な量, 14 寒い半分にいる割合,
/// 15・16 使っている中間ニューロンの数（1 層目・2 層目）, 17 一度に産む数,
/// 18〜24 直近 DEATH_WINDOW tick の死因ごとの死亡数（死因 1〜7: 餓死・寿命・食べられた・けが・病気・敵との争い・仲間との争い）
pub const LINEAGE_STATS: usize = 25;
/// 系統ごとの死因の内訳を数える期間（tick）
pub const DEATH_WINDOW: u64 = 1200;

/// 描画用バッファの 1 個体あたりの要素数: [x, y, 種類, 大きさ, 標識色, 向き]
pub const RENDER_STRIDE: usize = 6;

/// エネルギーの帳簿。f64 で積算して誤差を抑える
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    /// 開始時に世界にあった量（植物 + 個体）
    pub initial: f64,
    /// 日射から植物に入った量
    pub sunlight_in: f64,
    /// 代謝と移動で失われた量
    pub lost_metabolism: f64,
    /// 食べたもののうち吸収できずに失われた量
    pub lost_digestion: f64,
    /// 繁殖で失われた量
    pub lost_birth: f64,
    /// 寿命で死んだ個体が持っていた量（死体は今のところそのまま消える）
    pub lost_death: f64,
    /// 草食が植物から得た量（吸収できた分）。栄養段階の受け渡し効率の計測用
    pub herb_gain: f64,
    /// 肉食が獲物から得た量（吸収できた分）
    pub carn_gain: f64,
    /// 死体が腐って失われた量
    pub lost_decay: f64,
    /// 巣の材料にした植物の量（巣は熱を保つ構造物で、食べ物にはならない）
    pub lost_build: f64,
}

impl Ledger {
    /// 帳簿の上で、いま世界にあるはずの量
    pub fn expected_total(&self) -> f64 {
        self.initial + self.sunlight_in
            - self.lost_metabolism
            - self.lost_digestion
            - self.lost_birth
            - self.lost_death
            - self.lost_decay
            - self.lost_build
    }
}

/// 選択中の個体の観察記録。毎 tick の詳細は全個体ぶん持つと重いので、選択中の 1 体だけ記録する
#[derive(Default, Clone, Debug)]
pub struct Watch {
    /// 選択中の個体の id（0 は選択なし）
    pub id: u32,
    /// 直前の tick の移動速度
    pub speed: f32,
    /// 直前の tick に代謝と移動で失ったエネルギー
    pub cost: f32,
    /// 直前の tick の脳の出力
    pub output: [f32; genome::N_OUT],
    /// 直前の tick の欲求ごとの優先度（0〜1）。[食べる, 繁殖, 逃げる, 自由]
    pub priorities: [f32; genome::N_DRIVES],
    /// 直前の tick に曲がった角度（ラジアン、揺れを除く。正で右）
    pub turn: f32,
    /// 直前の tick の速さ（最高速度に対する割合）
    pub throttle: f32,
    /// 直前の tick に、繁殖相手が近くにいたか（無性生殖の設定なら常に true）
    pub mate_nearby: bool,
    /// 直前の tick の繁殖相手との相性（繁殖が成功する確率）。相手がいなければ負
    pub mate_compat: f32,
    /// 直前の tick の繁殖シグナルの強さ（0〜1）
    pub signal: f32,
    /// 直前の tick の体の調子（飢えと老いで最高速度に掛かる倍率、frail_speed〜1）
    pub vigor: f32,
    /// 直前の tick の脳の入力
    pub input: Vec<f32>,
    /// 直近 ENERGY_HISTORY tick のエネルギー（古い順）
    pub energy_history: Vec<f32>,
    /// 死んだときの (死因, tick)
    pub death: Option<(u8, u64)>,
}

/// 草食の大人の大きさの段階の境目（診断用）。段階は境目の数 ＋ 1 個
pub const HERB_SIZE_EDGES: [f32; 5] = [0.5, 0.65, 0.8, 1.0, 1.2];
pub const HERB_SIZE_BINS: usize = HERB_SIZE_EDGES.len() + 1;

/// 大人の大きさが入る段階
pub fn herb_size_bin(adult_size: f32) -> usize {
    HERB_SIZE_EDGES.iter().take_while(|&&e| adult_size >= e).count()
}

/// 肉食の大人の大きさの段階の境目（診断用）
pub const CARN_SIZE_EDGES: [f32; 5] = [0.9, 1.1, 1.3, 1.5, 1.7];
pub const CARN_SIZE_BINS: usize = CARN_SIZE_EDGES.len() + 1;

/// 肉食の大人の大きさが入る段階
pub fn carn_size_bin(adult_size: f32) -> usize {
    CARN_SIZE_EDGES.iter().take_while(|&&e| adult_size >= e).count()
}

/// 大きさの分布（adult_size_histogram）の段階の数
pub const SIZE_HIST_BINS: usize = 40;

/// 大人の大きさ s が入る分布の段階。genome::SIZE_RANGE を対数で等間隔に分ける（範囲の外は端の段階）
pub fn size_hist_bin(s: f32) -> usize {
    let (lo, hi) = genome::SIZE_RANGE;
    let t = (s.max(lo) / lo).ln() / (hi / lo).ln();
    ((t * SIZE_HIST_BINS as f32) as usize).min(SIZE_HIST_BINS - 1)
}

/// 起きたことの累計（調整・診断用）。添字 [種類] は HERBIVORE / CARNIVORE
#[derive(Default, Clone, Debug)]
pub struct Census {
    /// 死因ごとの死亡数 [種類][死因]
    pub deaths: [[u64; 8]; 2],
    /// そのうち、成熟する前に死んだ数 [種類][死因]
    pub juvenile_deaths: [[u64; 8]; 2],
    /// 同じ種類の敵への攻撃の回数 [種類]
    pub fights: [u64; 2],
    /// 仲間（同じ系統）への攻撃の回数 [種類]
    pub kin_fights: [u64; 2],
    /// 生まれた数 [種類]
    pub births: [u64; 2],
    /// 巣を作った回数（個体 × tick） [種類]
    pub builds: [u64; 2],
    /// 襲いかかった回数と、仕留めた回数
    pub hunts: u64,
    pub kills: u64,
    /// 襲いかかったときの、獲物の大きさ ÷ 自分の大きさ（協力込み）、獲物の近くの仲間の数、
    /// 近くの同じ系統の肉食の数、近くの系統の違う肉食の数、成功の見込み の合計
    pub hunt_ratio: f64,
    pub hunt_guards: f64,
    pub hunt_allies: f64,
    pub hunt_rivals: f64,
    pub hunt_chance: f64,
    /// 襲いかかったときの、獲物が遠ざかっていた速さ ÷ 肉食の最高速度（0〜1）の合計
    pub hunt_escape: f64,
    /// 襲われた獲物のうち、その tick の判断で天敵が見えていた数
    pub hunt_noticed: u64,
    /// 仕留められた獲物のうち、その tick の判断で天敵が見えていた数
    pub kill_noticed: u64,
    /// 仕留めた獲物を分け合った人数の合計
    pub kill_eaters: f64,
    /// 草食の大人の大きさの段階ごとの記録（herb_size_bin）。生まれた数は親（産んだ側）の段階で数える
    pub herb_size_births: [u64; HERB_SIZE_BINS],
    /// 死因ごとの死亡数 [段階][死因]
    pub herb_size_deaths: [[u64; 8]; HERB_SIZE_BINS],
    /// そのうち、成熟する前に死んだ数 [段階][死因]
    pub herb_size_juvenile_deaths: [[u64; 8]; HERB_SIZE_BINS],
    /// 肉食に襲われた回数 [段階]
    pub herb_size_attacked: [u64; HERB_SIZE_BINS],
    /// 肉食の大人の大きさの段階ごとの記録（carn_size_bin）。生まれた数は親（産んだ側）の段階で数える
    pub carn_size_births: [u64; CARN_SIZE_BINS],
    /// 死因ごとの死亡数 [段階][死因]
    pub carn_size_deaths: [[u64; 8]; CARN_SIZE_BINS],
    /// 襲った回数・仕留めた回数 [段階]
    pub carn_size_hunts: [u64; CARN_SIZE_BINS],
    pub carn_size_kills: [u64; CARN_SIZE_BINS],
    /// 襲ったときの 獲物の大きさ ÷ 自分の大きさ の合計 [段階]
    pub carn_size_ratio: [f64; CARN_SIZE_BINS],
    /// 狩りで得たエネルギーの合計（仲間と分けた分を含め、受け取った側の段階で数える） [段階]
    pub carn_size_gain: [f64; CARN_SIZE_BINS],
    /// 襲う費用の合計 [段階]
    pub carn_size_attack_cost: [f64; CARN_SIZE_BINS],
    /// 草を食べようとした回数 [段階]
    pub herb_size_grazes: [u64; HERB_SIZE_BINS],
    /// そのうち、その場所の草が一口に足りなかった回数 [段階]
    pub herb_size_graze_short: [u64; HERB_SIZE_BINS],
    /// 食べようとした一口の量の合計（今の大きさ・警戒・植生で決まる） [段階]
    pub herb_size_bite: [f64; HERB_SIZE_BINS],
    /// 実際に食べた量の合計 [段階]
    pub herb_size_take: [f64; HERB_SIZE_BINS],
    /// 生きる費用の内訳の合計 [種類][COST_NAMES の順]。観察用で、保存ファイルには入れない（読み込むと 0 から数え直す）
    pub costs: [[f64; 8]; 2],
    /// 繁殖できる状態（成熟・蓄え十分・休み明け）だった個体 × tick を、[繁殖を望まなかった, 望んだが相手がいなかった, 交配を試みた]
    /// に分けた数 [種類]。観察用で、保存ファイルには入れない
    pub ready: [[u64; 3]; 2],
    /// 子に与えた回数（個体 × tick）と、子の蓄えになった量 [種類]（観察用。保存しない）
    pub feeds: [u64; 2],
    pub fed: [f64; 2],
    /// 生まれたときの成長度の合計 [種類]（観察用。保存しない。births で割ると平均）
    pub birth_growth: [f64; 2],
}

/// step_deaths で、死体の奪い合いのけがで死んだことを表す死因（census の死因の集計では death::INJURED に数える）
pub const STEP_DEATH_CARRION_FIGHT: u8 = 8;

/// census.costs の項目の名前
pub const COST_NAMES: [&str; 8] = ["基礎代謝", "速さの維持", "体温", "シグナル", "目", "脳", "記憶", "移動"];

/// 欲求の番号（genome::mix_drives の並び）
const DRIVE_MATE: usize = 1;

/// 狩りの見込み。chance: 成功する確率、size_ratio: 獲物の大きさ ÷ 肉食の大きさ（協力込み）、guards: 獲物の近くの仲間の数
struct HuntOdds {
    chance: f32,
    /// 獲物が遠ざかっていた速さ ÷ 肉食の最高速度（0〜1）
    escape: f32,
    size_ratio: f32,
    guards: u32,
    allies: u32,
    rivals: u32,
}

/// 狩りの結果
enum Hunt {
    Caught,
    Failed,
    /// 届かない、または襲える状態でない
    NoAttempt,
}

/// 繁殖相手の見つかり具合
#[derive(Clone, Copy)]
enum Mate {
    /// 相手がいる（配列上の位置）
    Partner(usize),
    /// 相手が要らない（無性生殖の設定）
    Asexual,
    /// 相手が近くにいない
    Missing,
}

/// 1 回の知覚で見つけた相手。(配列上の位置, dx, dy, 距離の 2 乗)
type Seen = Option<(usize, f32, f32, f32)>;

#[derive(Clone, Copy)]
struct Senses {
    predator: Seen,
    prey: Seen,
    kin: Seen,
    /// 見えている仲間の数・向きの合計・興奮している数
    kin_count: u32,
    kin_hx: f32,
    kin_hy: f32,
    kin_alert: u32,
    /// 標識色が自分と近い仲間のうち最寄り、とその数
    similar: Seen,
    similar_count: u32,
    /// 繁殖相手の候補（成熟した仲間のうち、標識色が近い個体の最寄り。いなければ成熟した仲間の最寄り）
    mate: Seen,
    /// 近づきすぎた同じ種類の個体から離れる方向（近いほど強い）
    sep_dx: f32,
    sep_dy: f32,
    /// 見えている仲間の相対位置の合計（仲間の中心の方向を出すため）
    kin_dx: f32,
    kin_dy: f32,
    /// 興奮している（草食なら逃げている）仲間の向きの合計（警戒の合図）
    alarm_hx: f32,
    alarm_hy: f32,
    /// 見えている仲間の繁殖シグナルの合計
    kin_signal: f32,
    /// 見えている仲間のうち、自分の真後ろを視野に収めている個体の数
    rear_watchers: u32,
    /// 同じ種類の敵（系統色が離れている個体）のうち最寄り、とその数
    rival: Seen,
    rival_count: u32,
    /// 見えている獲物（好みの大きさの範囲のもの）の数（肉食のみ）
    prey_count: u32,
    /// 見えている親子（自分の親か、自分の子）のうち最寄り
    family: Seen,
    /// 自分の幼い子のうち最寄り（匂いや声でわかるので、隠れや視野角によらず視野の距離の内なら気づく）
    child: Seen,
}

/// 知覚と脳の判断（行動の前の段階）。tick の始めの世界だけを見て作るので、全個体を並列に作れる
#[derive(Clone, Copy)]
struct Plan {
    senses: Senses,
    input: [f32; N_IN],
    out: [f32; genome::N_OUT],
    /// 呼び声の大きい方向（自分基準の単位ベクトル）
    call_dir: (f32, f32),
    /// 更新後の「仲間を最後に見てからの tick 数」（行動の段階で書き込む）
    alone: u16,
}

/// 個体の生涯の記録（観察用）
#[derive(Clone, Copy, Default, Debug)]
pub struct Life {
    /// 食べて身についたエネルギーの合計（草・獲物・死体）
    pub eaten: f32,
    /// 仕留めた獲物の数（肉食）
    pub kills: u32,
    /// 襲いかかった回数（肉食）
    pub hunts: u32,
    /// 動いた距離の合計（px）
    pub distance: f32,
    /// 交配を試みた回数と、そのうち相性で失敗した回数
    pub matings: u32,
    pub failed: u32,
}

/// 個体が覚えている食べ物の場所（1 か所）
#[derive(Clone, Copy, Default, Debug)]
pub struct Memory {
    pub x: f32,
    pub y: f32,
    /// 覚えたときの良さ（0〜1）。0 なら何も覚えていない
    pub value: f32,
    /// 覚えた tick
    pub tick: u64,
}

/// 選択中の個体のエネルギーの履歴に残す tick 数
pub const ENERGY_HISTORY: usize = 300;

struct Birth {
    /// 親の id（繁殖を決めた側, 相手）。無性生殖なら相手は 0
    parents: [u32; 2],
    x: f32,
    y: f32,
    /// 親の位置（子の位置が水だったときに代わりに使う）
    parent_x: f32,
    parent_y: f32,
    kind: u8,
    energy: f32,
    generation: u32,
    /// births_genomes の中の開始位置
    genome_at: usize,
    /// 親が覚えている巣のマス（NO_HOME なら覚えていない）。子はこれを覚えて生まれる
    home: u32,
}

pub struct World {
    pub params: Params,
    pub width: f32,
    pub height: f32,
    pub tick: u64,

    /// 個体の通し番号。1 から始まり、生まれた順に増える
    pub id: Vec<u32>,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    /// 進行方向の単位ベクトル (hx, hy)。角度で持つと毎 tick 三角関数が要り、WASM では遅いので、ベクトルで持つ
    pub hx: Vec<f32>,
    pub hy: Vec<f32>,
    pub energy: Vec<f32>,
    pub age: Vec<u32>,
    pub kind: Vec<u8>,
    /// いまの状況（state::*）
    pub state: Vec<u8>,
    /// 初期個体が 0、その子が 1、…
    pub generation: Vec<u32>,
    pub children: Vec<u32>,
    /// 遺伝子。1 個体 GENOME_LEN 個ずつ詰めてある
    pub genomes: Vec<f32>,
    /// 遺伝子から読み出した体の形質。いまの大きさに関わる値は、成長に合わせて計算し直す
    pub traits: Vec<Traits>,
    /// 成長度（生まれたとき 0.2〜1、成熟すると 1）。1 になるまで繁殖できない
    pub growth: Vec<f32>,
    /// 次に襲えるようになるまでの残り tick 数（肉食のみ使う）
    pub cooldown: Vec<u16>,
    /// 次に繁殖できるようになるまでの残り tick 数（妊娠と子育てにあたる）
    pub rest: Vec<u16>,
    /// 敵に攻撃されて負けてからの残り tick 数（ATTACKED_TICKS から減る。0 なら最近は攻撃されていない）
    pub attacked: Vec<u8>,
    /// 仲間（同じ種類）を最後に見てからの tick 数
    pub alone: Vec<u16>,
    /// 直前の tick に脳が決めた警戒の度合い（0〜1、草食のみ使う）。天敵に気づける距離と一口の大きさに効く
    pub vigilance: Vec<f32>,
    /// 生まれた場所の植生（肥沃度）。草食はこれと大きく違う植生の場所では食べられない
    pub natal: Vec<f32>,
    /// tick の始めに求めた、足元の草の量（上限に対する割合）。隠れの判定に使う
    pub cover: Vec<f32>,
    /// 直前の tick に出した繁殖シグナルの強さ（0〜1）
    pub signal: Vec<f32>,
    /// 親の id（繁殖を決めた側, 相手）。初期個体は 0
    pub parents: Vec<[u32; 2]>,
    /// 生涯の記録
    pub life: Vec<Life>,
    /// 直前の step で死んだ個体の (id, 死因)。観察用で、保存ファイルには入れない（step の始めに空にする）。
    /// 死体の奪い合いのけがで死んだ個体は、死因を STEP_DEATH_CARRION_FIGHT にする
    pub step_deaths: Vec<(u32, u8)>,
    /// 覚えている食べ物の場所
    pub memory: Vec<Memory>,
    /// 覚えている巣のマス（植物のマスの番号。NO_HOME なら覚えていない）
    pub home: Vec<u32>,
    /// 最後の tick に実際に動いた量（世界の向きの (dx, dy)。動かなければ 0）
    pub vel: Vec<(f32, f32)>,
    /// 体力（0〜1）。巡航の速さを超えて走ると減り、それより遅ければ戻る
    pub stamina: Vec<f32>,
    /// 最後に子を産んだ場所と、それを覚えている最後の tick（子が育つまで。0 なら覚えていない）
    pub den: Vec<(f32, f32)>,
    pub den_until: Vec<u64>,
    /// 妊娠: 産む tick（0 なら妊娠していない）、お腹の子に渡したエネルギー、子の数、相手の id、相手の遺伝子の置き場所（NO_SLOT なら無性生殖）
    pub gest_due: Vec<u64>,
    pub gest_energy: Vec<f32>,
    pub gest_litter: Vec<u8>,
    pub gest_mate: Vec<u32>,
    gest_slot: Vec<u32>,
    /// 妊娠中の個体が交配した相手の遺伝子（GENOME_LEN ずつ。きょうだいは産むときにそれぞれ交叉する）と、空いている置き場所
    mate_genomes: Vec<f32>,
    free_slots: Vec<u32>,
    /// tick の始めに写した vel（前の tick の動き）。狩りで獲物が逃げていたかを見るのに使う（行動の順番で結果が変わらないように）
    prev_vel: Vec<(f32, f32)>,
    /// tick の途中で死んだ個体は false にしておき、tick の最後にまとめて取り除く
    alive: Vec<bool>,

    pub terrain: Terrain,
    pub plants: PlantField,
    /// マスごとの死体の量（植物と同じマス目）。死んだ個体の残したエネルギーで、少しずつ腐って減る。肉食の食べ物になる
    pub carrion: Vec<f32>,
    /// マスごとの死体の古さ（死んでからの tick 数。新しい死体が足されたら量で重み付けして平均する）
    pub carrion_age: Vec<f32>,
    /// マスごとの巣の出来の良さ（0〜1、植物と同じマス目）。作ると上がり、時間とともに崩れる
    pub nest: Vec<f32>,
    /// マスごとの巣の持ち主（最後に手を入れた個体の id。0 なら誰も作っていない）。記録のみで、誰でも使える
    pub nest_owner: Vec<u32>,
    pub ledger: Ledger,
    pub watch: Watch,

    /// JS に渡す描画用データ。fill_render() で更新する
    pub render: Vec<f32>,

    grid: Grid,
    /// 相手からの見えにくさ（足元の草から、繁殖シグナルで目立つ分を引いたもの）。グリッドに渡す作業用
    conceal: Vec<f32>,
    /// 動きの度合い（前の tick に動いた量 ÷ 最高速度、0〜1）。build_grid で作る
    motion: Vec<f32>,
    /// この tick の判断で天敵が見えていたか（草食）。狩りの集計に使う
    noticed: Vec<bool>,
    /// 呼び声の場: 成熟した個体の繁殖シグナルを、種類ごとに粗いマス目（call_cell 四方）に足し合わせたもの
    calls: [Vec<f32>; 2],
    call_cols: usize,
    call_rows: usize,
    rng: Rng,
    births: Vec<Birth>,
    births_genomes: Vec<f32>,
    next_id: u32,
    /// これまでの交配（相手がいて繁殖を試みた回数）と、そのうち相性で失敗した回数
    pub matings: u64,
    pub failed_matings: u64,
    pub census: Census,
    /// 長期の統計の記録（history.rs）
    pub history: History,
    /// 直近 DEATH_WINDOW tick の死亡の記録（古い順）。系統ごとの死因の内訳に使う
    recent_deaths: VecDeque<DeathRecord>,
    /// 世界を作ったときの乱数の種（保存した世界を読み込むときに地形を作り直すのに使う）
    pub seed: u64,
    /// 知覚と脳の判断を並列に計算するか（parallel 機能が有効なときだけ効く。結果は並列でも順番でも同じ）
    pub use_parallel: bool,
    /// 作業用: 処理する個体の順番（セル順）と、その判断
    order: Vec<usize>,
    plans: Vec<Plan>,
}

/// 死亡の記録（系統ごとの死因の内訳用）
#[derive(Clone, Copy)]
struct DeathRecord {
    tick: u64,
    kind: u8,
    cause: u8,
    hue: f32,
}

impl World {
    pub fn new(seed: u64, params: Params) -> Self {
        let mut world = World::empty(seed, params);
        let p = world.params.clone();
        for (kind, count, size, speed, vision, fov, energy, litter) in [
            (HERBIVORE, p.herb_initial, p.herb_initial_size, p.herb_initial_speed, p.herb_initial_vision, p.herb_initial_fov, p.herb_initial_energy, p.herb_initial_litter),
            (CARNIVORE, p.carn_initial, p.carn_initial_size, p.carn_initial_speed, p.carn_initial_vision, p.carn_initial_fov, p.carn_initial_energy, p.carn_initial_litter),
        ] {
            // 仲間の入力へのランダムな配線は、種類ごとに 1 回だけ引いて全員で共有する。
            // 個体ごとに引くと初期個体どうしの遺伝的距離が大きすぎて、最初から互いに繁殖できなくなる
            let mut g = genome::initial(kind == HERBIVORE, size, speed, vision, 0.0);
            g[genome::G_FOV] = fov.to_radians();
            g[genome::G_MEMORY] = p.memory_initial;
            g[genome::G_LITTER] = litter;
            genome::add_kin_wiring(&mut g, &mut world.rng);
            if p.memory_wiring != 0.0 {
                genome::add_memory_wiring(&mut g, p.memory_wiring);
            }
            if p.nest_wiring != 0.0 {
                genome::add_nest_wiring(&mut g, p.nest_wiring);
            }
            if kind == CARNIVORE && p.carn_family_wiring != 0.0 {
                genome::add_family_wiring(&mut g, p.carn_family_wiring);
            }
            if p.feed_wiring != 0.0 {
                genome::add_feed_wiring(&mut g, p.feed_wiring);
            }
            // 初期個体の組ごとの (一度に産む数, 子への投資)。組に分けないときは乱数を引かない
            let groups = p.founder_groups.max(0.0) as usize;
            let founders: Vec<(f32, f32)> = (0..groups)
                .map(|_| {
                    let litter = world.rng.range(p.founder_litter_min, p.founder_litter_max.max(p.founder_litter_min));
                    let invest = world.rng.range(p.founder_invest_min, p.founder_invest_max.max(p.founder_invest_min));
                    (litter, invest)
                })
                .collect();
            for n in 0..count as usize {
                let (x, y) = if groups > 0 {
                    // 組 k の個体は、東西に groups 等分した k 番目の帯に置く
                    let k = n % groups;
                    g[genome::G_LITTER] = founders[k].0;
                    g[genome::G_INVEST] = founders[k].1;
                    let band = world.width / groups as f32;
                    loop {
                        let (_, y) = world.random_position();
                        let x = band * (k as f32 + world.rng.f32());
                        if !world.terrain.is_water(x, y) {
                            break (x, y);
                        }
                    }
                } else {
                    world.random_position()
                };
                // 初期個体は 1 体ずつ違う標識色にする。系統の広がりが色で見える
                g[genome::G_HUE] = world.rng.f32();
                // 初期個体は、置かれた場所の気候に適応している
                g[genome::G_TEMP_PREF] = world.terrain.temperature_at(x, y);
                // 攻撃する傾向は初期個体ごとにばらつかせる（攻撃する個体がいないと、争いが得か損かを選択が試せない）
                let bias = p.initial_attack_bias + world.rng.gaussian() * p.initial_attack_spread;
                genome::set_attack_bias(&mut g, bias);
                let kin_bias = p.initial_kin_attack_bias + world.rng.gaussian() * p.initial_kin_attack_spread;
                genome::set_kin_attack_bias(&mut g, kin_bias);
                world.spawn(x, y, kind, energy, 0, &g);
                // 初期の世代がそろって老いないよう、年齢をばらつかせる（大人として置くので成長度はそのまま）。
                // 0 のときは乱数を引かない（乱数の流れを、この仕組みがなかったときと同じにする）
                if p.initial_age_spread > 0.0 {
                    let base_age = if kind == HERBIVORE { p.herb_max_age } else { p.carn_max_age };
                    let last = world.count() - 1;
                    world.age[last] = (world.rng.f32() * p.initial_age_spread * base_age) as u32;
                }
            }
        }
        world.ledger.initial = world.total_energy();
        world
    }

    /// 個体のいない世界（地形・植物・死体の場・呼び声の場）を作る。乱数の流れは seed から始まる
    fn empty(seed: u64, params: Params) -> Self {
        let width = params.world_width;
        let height = params.world_height;
        // 地形は個体とは別の乱数の流れで作る（地形の作り方を変えても、個体の歴史の乱数がずれないように）
        let terrain = if params.water_fraction <= 0.0 && params.fertility_min >= 1.0 {
            Terrain::flat(width, height, params.plant_cell, params.temp_mean)
        } else {
            let spec = TerrainSpec {
                water_fraction: params.water_fraction,
                elevation_scale: params.elevation_scale,
                fertility_scale: params.fertility_scale,
                fertility_min: params.fertility_min,
                temp_mean: params.temp_mean,
                temp_latitude: params.temp_latitude,
                temp_lapse: params.temp_lapse,
                temp_noise: params.temp_noise,
                plant_temp_opt: params.plant_temp_opt,
                plant_temp_width: params.plant_temp_width,
                season_temp: params.season_temp,
            };
            Terrain::generate(width, height, params.plant_cell, &spec, &mut Rng::new(seed ^ 0x7e44_a1d3))
        };
        let plants = PlantField::new(
            width,
            height,
            params.plant_cell,
            params.plant_capacity * params.plant_initial_ratio,
            &terrain.fertility,
            &terrain.soil,
            params.plant_shares(),
            params.woodland_contrast,
        );
        let grid = Grid::new(width, height, GRID_CELL);
        let mut world = World {
            width,
            height,
            tick: 0,
            id: Vec::new(),
            xs: Vec::new(),
            ys: Vec::new(),
            hx: Vec::new(),
            hy: Vec::new(),
            energy: Vec::new(),
            age: Vec::new(),
            kind: Vec::new(),
            state: Vec::new(),
            generation: Vec::new(),
            children: Vec::new(),
            genomes: Vec::new(),
            traits: Vec::new(),
            growth: Vec::new(),
            cooldown: Vec::new(),
            rest: Vec::new(),
            attacked: Vec::new(),
            alone: Vec::new(),
            memory: Vec::new(),
            home: Vec::new(),
            vel: Vec::new(),
            stamina: Vec::new(),
            den: Vec::new(),
            den_until: Vec::new(),
            gest_due: Vec::new(),
            gest_energy: Vec::new(),
            gest_litter: Vec::new(),
            gest_mate: Vec::new(),
            gest_slot: Vec::new(),
            mate_genomes: Vec::new(),
            free_slots: Vec::new(),
            prev_vel: Vec::new(),
            vigilance: Vec::new(),
            natal: Vec::new(),
            cover: Vec::new(),
            signal: Vec::new(),
            parents: Vec::new(),
            life: Vec::new(),
            step_deaths: Vec::new(),
            alive: Vec::new(),
            terrain,
            carrion: vec![0.0; plants.biomass.len()],
            carrion_age: vec![0.0; plants.biomass.len()],
            nest: vec![0.0; plants.biomass.len()],
            nest_owner: vec![0; plants.biomass.len()],
            plants,
            ledger: Ledger::default(),
            history: History::new(),
            watch: Watch::default(),
            render: Vec::new(),
            grid,
            conceal: Vec::new(),
            motion: Vec::new(),
            noticed: Vec::new(),
            calls: [Vec::new(), Vec::new()],
            call_cols: 0,
            call_rows: 0,
            rng: Rng::new(seed),
            births: Vec::new(),
            births_genomes: Vec::new(),
            next_id: 1,
            matings: 0,
            failed_matings: 0,
            census: Census::default(),
            recent_deaths: VecDeque::new(),
            seed,
            use_parallel: true,
            order: Vec::new(),
            plans: Vec::new(),
            params,
        };
        world.call_cols = ((width / world.params.call_cell).ceil() as usize).max(1);
        world.call_rows = ((height / world.params.call_cell).ceil() as usize).max(1);
        world.calls = [vec![0.0; world.call_cols * world.call_rows], vec![0.0; world.call_cols * world.call_rows]];
        // 季節の始め（季節 0）の気温と植物の育ちやすさ。平均気温と同じなので、初期個体の好みの気温はこれに合わせてよい
        world.update_climate();
        world
    }

    /// 陸の上のランダムな位置
    fn random_position(&mut self) -> (f32, f32) {
        loop {
            let (x, y) = (self.rng.range(0.0, self.width), self.rng.range(0.0, self.height));
            if !self.terrain.is_water(x, y) {
                return (x, y);
            }
        }
    }

    /// 成長しきった大人として加える
    fn spawn(&mut self, x: f32, y: f32, kind: u8, energy: f32, generation: u32, genome: &[f32]) {
        let traits = genome::traits(genome, self.params.max_turn);
        self.spawn_grown(x, y, kind, energy, generation, genome, traits, 1.0);
    }

    /// 成長度 growth の個体として加える。traits は genome から読み出したもの
    #[allow(clippy::too_many_arguments)]
    fn spawn_grown(
        &mut self,
        x: f32,
        y: f32,
        kind: u8,
        energy: f32,
        generation: u32,
        genome: &[f32],
        mut traits: Traits,
        growth: f32,
    ) {
        traits.grow_to(growth, self.params.max_turn);
        self.id.push(self.next_id);
        self.next_id += 1;
        self.xs.push(x);
        self.ys.push(y);
        let (sin, cos) = self.rng.range(0.0, std::f32::consts::TAU).sin_cos();
        self.hx.push(cos);
        self.hy.push(sin);
        self.energy.push(energy);
        self.age.push(0);
        self.kind.push(kind);
        self.state.push(state::WANDER);
        self.generation.push(generation);
        self.children.push(0);
        self.genomes.extend_from_slice(genome);
        self.traits.push(traits);
        self.growth.push(growth);
        self.cooldown.push(0);
        self.rest.push(0);
        self.attacked.push(0);
        self.alone.push(0);
        self.memory.push(Memory::default());
        self.home.push(NO_HOME);
        self.vel.push((0.0, 0.0));
        self.stamina.push(1.0);
        self.den.push((0.0, 0.0));
        self.den_until.push(0);
        self.gest_due.push(0);
        self.gest_energy.push(0.0);
        self.gest_litter.push(0);
        self.gest_mate.push(0);
        self.gest_slot.push(NO_SLOT);
        self.vigilance.push(0.5);
        self.natal.push(self.terrain.soil_at(x, y));
        self.cover.push(0.0);
        self.signal.push(0.0);
        self.parents.push([0, 0]);
        self.life.push(Life::default());
        self.alive.push(true);
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    pub fn count_kind(&self, kind: u8) -> usize {
        self.kind.iter().filter(|&&k| k == kind).count()
    }

    pub fn genome(&self, i: usize) -> &[f32] {
        &self.genomes[i * GENOME_LEN..(i + 1) * GENOME_LEN]
    }

    /// 世界にあるエネルギーの実際の合計（植物 + 個体）
    pub fn total_energy(&self) -> f64 {
        self.plants.total()
            + self.carrion.iter().map(|&c| c as f64).sum::<f64>()
            + self.energy.iter().map(|&e| e as f64).sum::<f64>()
            + (0..self.count()).map(|i| self.body_energy(i) as f64).sum::<f64>()
    }

    /// id の個体の配列上の位置。いなければ None
    pub fn index_of(&self, id: u32) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.id.iter().position(|&v| v == id)
    }

    /// (x, y) から radius 以内で最も近い個体の id。いなければ 0
    pub fn id_near(&self, x: f32, y: f32, radius: f32) -> u32 {
        self.id_near_kind(x, y, radius, None)
    }

    /// (x, y) から radius 以内で最も近い、種類 kind の個体の id（kind が None なら種類を問わない）。いなければ 0
    pub fn id_near_kind(&self, x: f32, y: f32, radius: f32, kind: Option<u8>) -> u32 {
        let mut best = radius * radius;
        let mut found = 0;
        for i in 0..self.count() {
            if kind.is_some_and(|k| self.kind[i] != k) {
                continue;
            }
            let dx = torus_delta(self.xs[i] - x, self.width);
            let dy = torus_delta(self.ys[i] - y, self.height);
            let d2 = dx * dx + dy * dy;
            if d2 < best {
                best = d2;
                found = self.id[i];
            }
        }
        found
    }

    /// 個体を選択して観察を始める（0 で選択解除）
    /// id の個体の、生きている子の数
    pub fn living_children(&self, id: u32) -> u32 {
        if id == 0 {
            return 0;
        }
        self.parents.iter().filter(|p| p[0] == id || p[1] == id).count() as u32
    }

    pub fn select(&mut self, id: u32) {
        self.watch = Watch { id, ..Watch::default() };
    }

    // ---- 種類ごとに値が変わるパラメータ（大きさによる補正込み） ----

    fn base_repro_threshold(&self, kind: u8) -> f32 {
        if kind == HERBIVORE { self.params.herb_repro_threshold } else { self.params.carn_repro_threshold }
    }

    /// 繁殖に必要なエネルギー（大人の大きさに比例）
    pub fn repro_threshold(&self, i: usize) -> f32 {
        self.base_repro_threshold(self.kind[i]) * self.traits[i].adult_size
    }

    /// 体そのものが持つエネルギー（body_energy × 大人の大きさ × 成長度）
    pub fn body_energy(&self, i: usize) -> f32 {
        self.params.body_energy * self.traits[i].adult_size * self.growth[i] + self.gest_energy[i]
    }

    /// お腹の子の重さ（gestation_load × お腹の子のエネルギー ÷ 大人の体のエネルギー）。妊娠していなければ 0
    pub fn gestation_load(&self, i: usize) -> f32 {
        if self.gest_energy[i] <= 0.0 {
            return 0.0;
        }
        self.params.gestation_load * self.gest_energy[i] / (self.params.body_energy * self.traits[i].adult_size).max(1e-6)
    }

    /// 繁殖できる状態か（成熟していて、エネルギーが足りていて、前の繁殖から十分たっている）
    /// 動ける度合い（0〜1）。成長度が immobile_growth 以下の子は動けず、自分では食べられない。mobile_growth 以上なら 1
    pub fn mobility(&self, i: usize) -> f32 {
        let (lo, hi) = (self.params.immobile_growth, self.params.mobile_growth);
        let g = self.growth[i];
        if hi <= lo {
            return if g > lo { 1.0 } else { 0.0 };
        }
        let x = ((g - lo) / (hi - lo)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }

    /// 親 i が子 j に蓄えを分け与える。渡した量の feed_efficiency 倍が子の蓄えになり、残りは熱として失う。与えたら true
    fn feed(&mut self, i: usize, j: usize) -> bool {
        let p = &self.params;
        let eff = p.feed_efficiency.clamp(0.01, 1.0);
        let room = (self.energy_cap(j) - self.energy[j]).max(0.0);
        let amount = (p.feed_rate * self.traits[i].size_pow075).min(self.energy[i]).min(room / eff);
        if amount <= 0.0 {
            return false;
        }
        self.energy[i] -= amount;
        self.energy[j] += amount * eff;
        self.ledger.lost_metabolism += (amount * (1.0 - eff)) as f64;
        let k = self.kind[i] as usize;
        self.census.feeds[k] += 1;
        self.census.fed[k] += (amount * eff) as f64;
        true
    }

    pub fn can_breed(&self, i: usize) -> bool {
        self.growth[i] >= 1.0 && self.energy[i] >= self.repro_threshold(i) && self.rest[i] == 0 && self.gest_due[i] == 0
    }

    /// 蓄えられるエネルギーの上限（いまの大きさに比例。幼体は小さい）
    pub fn energy_cap(&self, i: usize) -> f32 {
        self.base_repro_threshold(self.kind[i]) * self.traits[i].size * self.params.energy_cap_ratio
    }

    /// 個体 i（草食）が、植生の種類 fertility（土の肥えかた）の草をどれだけ食べられるか（0〜1）。
    /// 生まれた場所の植生との差が habitat_tolerance 以下なら 1、habitat_limit 以上なら 0。肉食は常に 1
    pub fn habitat_fit(&self, i: usize, fertility: f32) -> f32 {
        if self.kind[i] != HERBIVORE {
            return 1.0;
        }
        let p = &self.params;
        let diff = (fertility - self.natal[i]).abs();
        let t = ((diff - p.habitat_tolerance) / (p.habitat_limit - p.habitat_tolerance).max(1e-6)).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    }

    /// 季節に合わせて、今の気温と植物の育ちやすさを更新する
    fn update_climate(&mut self) {
        let season = self.season();
        self.terrain.set_season(season);
        let p = &self.params;
        let rain = RainSpec { dry: p.rain_dry, width: p.rain_width, center: p.rain_center, swing: p.rain_swing };
        self.terrain.set_rain(season, &rain);
        self.terrain.productivity_into(&mut self.plants.fertility);
    }

    /// 1 年のうちの位置（0〜1）。0.25 で日射が最も多く、0.75 で最も少ない。季節がない設定なら 0
    pub fn season_phase(&self) -> f32 {
        let len = self.params.season_length;
        if len <= 0.0 {
            return 0.0;
        }
        (self.tick as f64 / len as f64).fract() as f32
    }

    /// 季節（−1〜1）。日射が最も多い時期に 1、最も少ない時期に −1。季節がない設定なら 0
    pub fn season(&self) -> f32 {
        if self.params.season_length <= 0.0 {
            return 0.0;
        }
        (std::f32::consts::TAU * self.season_phase()).sin()
    }

    /// 寿命（tick）。大人の大きさの 0.25 乗に比例し、個体ごとに 0.75〜1.25 倍にばらつく。
    /// 全員が同じ寿命だと、同じ時期に生まれた世代がそろって死に、個体数の波が強まる。
    /// ばらつきは id から決める（乱数の流れを変えず、個体ごとの記録も増やさない）
    pub fn lifespan(&self, i: usize) -> f32 {
        let p = &self.params;
        let base = if self.kind[i] == HERBIVORE { p.herb_max_age } else { p.carn_max_age };
        // id をかき混ぜて 0〜1 にする（splitmix の混ぜ方）
        let mut h = (self.id[i] as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        h ^= h >> 31;
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 29;
        let u = (h >> 40) as f32 / (1u64 << 24) as f32;
        base * self.traits[i].adult_pow025 * (0.75 + 0.5 * u)
    }

    /// 巡航の速さ（最高速度に対する割合）
    fn cruise(&self, i: usize) -> f32 {
        if self.kind[i] == HERBIVORE { self.params.herb_cruise } else { self.params.carn_cruise }
    }

    /// 今の体力で出せる速さの上限（最高速度に対する割合）。体力が STAMINA_FADE 以上なら 1、0 で巡航の速さ
    pub fn stamina_cap(&self, i: usize) -> f32 {
        let cruise = self.cruise(i);
        cruise + (1.0 - cruise) * (self.stamina[i] / STAMINA_FADE).min(1.0)
    }

    /// 動いた速さで体力を増減する。巡航の速さを超えた分だけ減り（全力なら stamina_sprint tick で尽きる）、
    /// 巡航より遅ければ遅いほど早く戻る（止まっていれば stamina_recover tick で満ちる）
    fn tire(&mut self, i: usize, speed: f32) {
        let p = &self.params;
        let cruise = self.cruise(i);
        let effort = (speed / self.traits[i].max_speed.max(1e-6)).clamp(0.0, 1.0);
        let change = if effort > cruise {
            -(effort - cruise) / (1.0 - cruise).max(1e-6) / p.stamina_sprint.max(1.0)
        } else {
            (cruise - effort) / cruise.max(1e-6) / p.stamina_recover.max(1.0)
        };
        self.stamina[i] = (self.stamina[i] + change).clamp(0.0, 1.0);
    }

    /// 体の調子（最高速度に掛かる倍率、frail_speed〜1）。飢えているほど、寿命に近いほど遅くなり、弱いほうの値を使う
    pub fn vigor(&self, i: usize) -> f32 {
        let p = &self.params;
        let low = p.frail_speed;
        let fed = self.energy[i] / self.repro_threshold(i);
        let hunger = if fed < p.frail_energy { low + (1.0 - low) * (fed / p.frail_energy).max(0.0) } else { 1.0 };
        let aged = self.age[i] as f32 / self.lifespan(i);
        let old = if aged > p.frail_age {
            1.0 - (1.0 - low) * ((aged - p.frail_age) / (1.0 - p.frail_age).max(1e-6)).min(1.0)
        } else {
            1.0
        };
        hunger.min(old)
    }

    /// (x, y) のマスに死体を amount だけ置く
    fn drop_carrion(&mut self, x: f32, y: f32, amount: f32) {
        if amount > 0.0 {
            let c = self.plants.cell_index(x, y);
            // 古さは量で重み付けした平均（新しい死体は古さ 0）
            let total = self.carrion[c] + amount;
            self.carrion_age[c] *= self.carrion[c] / total;
            self.carrion[c] = total;
        }
    }

    /// 死体を腐らせる。腐った分は帳簿の lost_decay に入る。CARRION_MIN を切った死体は消す
    fn decay_carrion(&mut self) {
        let rate = self.params.carrion_decay;
        let mut lost = 0.0f64;
        for (c, age) in self.carrion.iter_mut().zip(self.carrion_age.iter_mut()) {
            if *c <= 0.0 {
                continue;
            }
            let d = if *c < CARRION_MIN { *c } else { *c * rate };
            *c -= d;
            lost += d as f64;
            *age = if *c > 0.0 { *age + 1.0 } else { 0.0 };
        }
        self.ledger.lost_decay += lost;
    }

    /// 巣を崩す（出来の良さを nest_decay の割合で下げ、NEST_MIN を切ったら消す）。巣はエネルギーを持たないので帳簿は動かない
    fn decay_nests(&mut self) {
        let keep = 1.0 - self.params.nest_decay;
        for (q, owner) in self.nest.iter_mut().zip(self.nest_owner.iter_mut()) {
            if *q <= 0.0 {
                continue;
            }
            *q *= keep;
            if *q < NEST_MIN {
                *q = 0.0;
                *owner = 0;
            }
        }
    }

    /// 個体 i が今いるマスで巣を作る。茂みと枝を材料にして出来の良さを上げ、エネルギーを使う。
    /// 1 tick 分の材料がない、または巣がもう出来上がっていれば作らず false（わずかに残った材料を拾い続けて止まり続けることがないように）
    fn build_nest(&mut self, i: usize) -> bool {
        let c = self.plants.cell_index(self.xs[i], self.ys[i]);
        let p = &self.params;
        let room = 1.0 - self.nest[c];
        let (bush, branch) = (self.plants.layer[1][c].max(0.0), self.plants.layer[2][c].max(0.0));
        let size = self.traits[i].size;
        let want = (p.nest_material * size).min(room * p.nest_material_need);
        if want <= 1e-6 || bush + branch < want {
            return false;
        }
        let take = want;
        // 茂みと枝から、ある量に比例して取る
        let from_bush = take * bush / (bush + branch);
        self.plants.take(c, 1, from_bush);
        self.plants.take(c, 2, take - from_bush);
        self.ledger.lost_build += take as f64;
        self.nest[c] = (self.nest[c] + take / p.nest_material_need).min(1.0);
        self.nest_owner[c] = self.id[i];
        self.home[i] = c as u32;
        let cost = (p.nest_build_cost * self.traits[i].size_pow075).min(self.energy[i]);
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.census.builds[self.kind[i] as usize] += 1;
        true
    }

    /// 周り 8 マスのうち、いまいるマスより死体が多いマスの方向（世界の向き、単位ベクトルでない）
    fn carrion_direction(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (cols, rows) = (self.plants.cols, self.plants.rows);
        let c = ((x / self.plants.cell_size) as usize).min(cols - 1);
        let r = ((y / self.plants.cell_size) as usize).min(rows - 1);
        // 新しい死体の多い方へ（量 × 新しさ で比べる）
        let value = |k: usize| self.carrion[k] * self.carrion_freshness(k);
        let mut best = value(r * cols + c) + 0.05;
        let mut dir = None;
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let rr = (r as i32 + dr).rem_euclid(rows as i32) as usize;
                let cc = (c as i32 + dc).rem_euclid(cols as i32) as usize;
                let v = value(rr * cols + cc);
                if v > best {
                    best = v;
                    dir = Some((dc as f32, dr as f32));
                }
            }
        }
        dir
    }

    /// マス c の死体の新しさ（0〜1）
    fn carrion_freshness(&self, c: usize) -> f32 {
        (-self.carrion_age[c] / self.params.carrion_spoil.max(1e-6)).exp()
    }

    /// 肉食 i が足元の死体を食べる。食べたら true。
    /// 古い死体ほど身につく割合が下がり、病気になりやすい。近くに系統の違う肉食がいると一口が減り、奪い合いでけがをすることがある
    fn scavenge(&mut self, i: usize) -> bool {
        let room = self.energy_cap(i) - self.energy[i];
        let c = self.plants.cell_index(self.xs[i], self.ys[i]);
        if room <= 0.0 || self.carrion[c] <= 0.0 {
            return false;
        }
        let fresh = self.carrion_freshness(c);
        let (rivals, _) = self.carnivore_neighbors(i);
        let p = &self.params;
        let eff = p.carn_efficiency * fresh;
        let bite = p.carn_scavenge_bite * self.traits[i].size / (1.0 + p.carn_interference * rivals as f32) * self.mobility(i);
        let take = bite.min(self.carrion[c]).min(room / eff.max(1e-6));
        self.carrion[c] -= take;
        let gain = take * eff;
        self.energy[i] += gain;
        self.life[i].eaten += gain;
        self.ledger.carn_gain += gain as f64;
        self.ledger.lost_digestion += (take - gain) as f64;
        // 腐った死体による病気と、奪い合いによるけが
        let (sick_chance, sick_damage) = (p.carrion_sick * (1.0 - fresh), p.carrion_sick_damage);
        let (fight_chance, fight_damage) = (1.0 - (1.0 - p.carrion_fight).powi(rivals as i32), p.carrion_fight_damage);
        if self.rng.f32() < sick_chance {
            self.hurt(i, sick_damage, death::SICK);
        }
        if self.alive[i] && self.rng.f32() < fight_chance {
            self.hurt(i, fight_damage, death::INJURED);
            // 観察用: 死因の集計（census）では反撃と同じ「けが」だが、step_deaths では死体の奪い合いを分けて記録する
            if !self.alive[i]
                && let Some(last) = self.step_deaths.last_mut()
            {
                last.1 = STEP_DEATH_CARRION_FIGHT;
            }
        }
        true
    }

    /// 草食 i の周り 3 × 3 マスの、自分が届く草の量 × 質（上限に対する割合の合計、0〜9）
    fn plants_around(&self, i: usize) -> f32 {
        let (cols, rows) = (self.plants.cols, self.plants.rows);
        let c = ((self.xs[i] / self.plants.cell_size) as usize).min(cols - 1);
        let r = ((self.ys[i] / self.plants.cell_size) as usize).min(rows - 1);
        let cap = self.params.plant_capacity;
        let (reach, floor) = (self.reach(i), self.graze_floor(i));
        let mut sum = 0.0;
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                let rr = (r as i32 + dr).rem_euclid(rows as i32) as usize;
                let cc = (c as i32 + dc).rem_euclid(cols as i32) as usize;
                sum += (self.food_value(&reach, floor, rr * cols + cc) / cap).min(1.0);
            }
        }
        sum
    }

    /// 個体 i が植物の層ごとに届く割合（0〜1）。届く高さは体長（今の大きさの 1/3 乗）。
    /// 層ごとに「届き始め」から reach_width の幅で、なめらかに 0 から 1 に増える。地面の草には誰でも届く
    pub fn reach(&self, i: usize) -> [f32; LAYERS] {
        let p = &self.params;
        let length = self.traits[i].size.max(0.0).cbrt();
        let w = p.reach_width.max(1e-6);
        let reach_from = |start: f32| ((length - start) / w).clamp(0.0, 1.0);
        [1.0, reach_from(p.reach_shrub), reach_from(p.reach_tree)]
    }

    /// 個体 i が地面の草を食べ残す高さ（地面の層の上限に対する割合、graze_floor_min〜0.95）。大きい口ほど短い草を噛み取れない
    pub fn graze_floor(&self, i: usize) -> f32 {
        let p = &self.params;
        (p.graze_floor * self.traits[i].size.max(0.0).powf(p.graze_floor_power)).clamp(p.graze_floor_min.clamp(0.0, 0.95), 0.95)
    }

    /// 遠くの景色の区画 b の、届く割合が reach・食べ残す高さが floor・生まれた植生が natal の草食にとっての価値
    /// （取れる量 × 質 × 植生の合い具合。植物量と同じ尺度の、区画のマスあたりの平均）
    fn landscape_value(&self, i: usize, reach: &[f32; LAYERS], floor: f32, b: usize) -> f32 {
        let ls = &self.plants.landscape;
        if ls.vegetation[b] < 0.0 {
            return 0.0; // 陸がない
        }
        let q = self.params.plant_qualities();
        let cap = self.params.plant_capacity * ls.ground_cap[b];
        let ground = (ls.layer[0][b] - floor * cap).max(0.0);
        let food = q[0] * ground + (1..LAYERS).map(|k| q[k] * reach[k] * ls.layer[k][b]).sum::<f32>();
        food * self.habitat_fit(i, ls.vegetation[b])
    }

    /// 草食 i が遠くの景色で見つけた、食べ物の多い場所の方向（世界の向きの単位ベクトル）。
    /// 見渡せる距離（far_view × 体長。林の中では枝に遮られて縮む）の 8 方向の区画を比べ、今いる区画より十分に多いものがなければ None
    fn far_food_direction(&self, i: usize, reach: &[f32; LAYERS], floor: f32) -> Option<(f32, f32)> {
        if self.plants.landscape.cols == 0 {
            return None;
        }
        let (x, y) = (self.xs[i], self.ys[i]);
        let range = self.params.far_view * self.traits[i].size.max(0.0).cbrt() * self.view_openness(x, y);
        let here = self.landscape_value(i, reach, floor, self.plants.landscape_block(x, y));
        let mut best = here * 1.2 + 0.05;
        let mut dir = None;
        const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
        for (dx, dy) in [(1.0, 0.0), (D, D), (0.0, 1.0), (-D, D), (-1.0, 0.0), (-D, -D), (0.0, -1.0), (D, -D)] {
            let v = self.landscape_value(i, reach, floor, self.plants.landscape_block(x + dx * range, y + dy * range));
            if v > best {
                best = v;
                dir = Some((dx, dy));
            }
        }
        dir
    }

    /// 場所 (x, y) の見通しのよさ（遠くを見渡せる距離に掛ける倍率）。枝の層の割合が多いほど低く、全体の平均の割合の場所で 1
    pub fn view_openness(&self, x: f32, y: f32) -> f32 {
        let k = self.params.far_view_tree.max(0.0);
        let tree = self.plants.share_at(self.plants.cell_index(x, y))[2];
        let mean = self.plants.share[2];
        ((1.0 - k * tree).max(0.1) / (1.0 - k * mean).max(0.1)).max(0.0)
    }

    /// 個体が層 k から取れる量。地面の草は、食べ残す高さ floor（割合）より上の分だけ
    fn layer_available(&self, reach: &[f32; LAYERS], floor: f32, k: usize, c: usize) -> f32 {
        let amount = self.plants.layer[k][c];
        if k == 0 {
            let cap = self.params.plant_capacity * self.plants.ground_cap(c);
            (amount - floor * cap).max(0.0)
        } else {
            reach[k] * amount
        }
    }

    /// マス c の植物の、届く割合が reach・食べ残す高さが floor の個体にとっての価値（取れる量 × 質 の合計。植物量と同じ尺度）
    fn food_value(&self, reach: &[f32; LAYERS], floor: f32, c: usize) -> f32 {
        let q = self.params.plant_qualities();
        (0..LAYERS).map(|k| q[k] * self.layer_available(reach, floor, k, c)).sum()
    }

    /// 個体 i が覚えている場所の今の良さ（覚えたときの良さ × 薄れ具合 exp(−経過 / 記憶の長さ)）
    pub fn recalled_value(&self, i: usize) -> f32 {
        let m = self.memory[i];
        if m.value <= 0.0 {
            return 0.0;
        }
        let elapsed = self.tick.saturating_sub(m.tick) as f32;
        m.value * (-elapsed / self.traits[i].memory).exp()
    }

    /// 全個体の脳への入力（観察用）。tick の始めと同じ下ごしらえ（足元の草・遠くの景色・近傍探索・呼び声）をしてから知覚する。
    /// 世界の歴史は変えない（下ごしらえは次の step でも同じ値に作り直される）
    pub fn brain_inputs(&mut self) -> Vec<[f32; N_IN]> {
        let cap = self.params.plant_capacity;
        for i in 0..self.count() {
            self.cover[i] = self.plants.ratio_at(self.xs[i], self.ys[i], cap);
        }
        self.plants.update_landscape();
        self.build_grid();
        self.build_calls();
        (0..self.count()).map(|i| self.plan(i).input).collect()
    }

    /// 行動の段の後に、今いる場所の良さ value（0〜1）を記憶と比べる。覚えている場所の今の良さ以上なら、今いる場所を覚え直す。
    /// 食べて減っていく場所では今の良さが記憶より下がるので覚え直さず、来たときの良さが残る。
    /// 戻って来て悪くなっていても記憶は下げない（下げると、食べながら記憶が足元の減った値に追いつき、離れた場所を指さなくなる）
    fn remember(&mut self, i: usize, value: f32) {
        if value > 0.0 && value >= self.recalled_value(i) {
            self.memory[i] = Memory { x: self.xs[i], y: self.ys[i], value, tick: self.tick };
        }
    }

    /// 個体 i の近く（coop_radius 以内）にいる、同じ種類で系統色の近い（同じ系統の）生きている個体の数
    fn allies_of(&self, i: usize) -> u32 {
        let p = &self.params;
        let (x, y, kind, hue) = (self.xs[i], self.ys[i], self.kind[i], self.traits[i].hue);
        let r2 = p.coop_radius * p.coop_radius;
        let mut n = 0;
        self.grid.for_each_within(x, y, p.coop_radius, |k, xk, yk, kind_k, _| {
            if k == i || kind_k != kind || !self.alive[k] {
                return;
            }
            let dx = torus_delta(xk - x, self.width);
            let dy = torus_delta(yk - y, self.height);
            if dx * dx + dy * dy < r2 && genome::hue_distance(hue, self.traits[k].hue) <= p.hue_similar {
                n += 1;
            }
        });
        n
    }

    /// 争いでの強さ: いまの大きさ × (1 + 協力の強さ × 近くの同じ系統の仲間の数（上限あり）)
    fn fight_strength(&self, i: usize) -> f32 {
        let p = &self.params;
        let allies = (self.allies_of(i) as f32).min(p.coop_max_allies);
        self.traits[i].size * (1.0 + p.coop_strength * allies)
    }

    /// 個体 i が同じ種類の敵 j を攻撃する。勝つ確率は強さの fight_size_power 乗の比（既定では 2 乗の比）。
    /// 負けた側は勝った側の大きさに応じたけがをし、しばらく「攻撃された」状態になる。勝った側も小さなけがをする
    fn fight(&mut self, i: usize, j: usize, cause: u8) {
        let cost = self.params.fight_cost.min(self.energy[i]);
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.cooldown[i] = self.params.attack_cooldown as u16;
        let (si, sj) = (self.fight_strength(i), self.fight_strength(j));
        let k = self.params.fight_size_power;
        let (pi, pj) = (si.powf(k), sj.powf(k));
        let win = self.rng.f32() < pi / (pi + pj).max(1e-6);
        let (winner, loser) = if win { (i, j) } else { (j, i) };
        let (p_damage, p_backlash) = (self.params.fight_damage, self.params.fight_backlash);
        let damage = p_damage * self.traits[winner].size;
        let backlash = p_damage * self.traits[loser].size * p_backlash;
        self.attacked[loser] = ATTACKED_TICKS;
        self.hurt(loser, damage, cause);
        self.hurt(winner, backlash, cause);
    }

    /// 個体 i がけがや病気でエネルギー damage を失う。手持ちを超えたら死因 cause で死ぬ（手持ちを使い切って死ぬので、死体は残らない）
    fn hurt(&mut self, i: usize, damage: f32, cause: u8) {
        let lost = damage.min(self.energy[i]);
        self.energy[i] -= lost;
        self.ledger.lost_metabolism += lost as f64;
        if self.energy[i] <= 0.0 {
            self.energy[i] = 0.0;
            self.die(i, cause);
        }
    }

    /// 近傍探索のグリッドを作り直す。相手からの見えにくさ（グリッドに渡す値）は 1 − 見つかりやすさ。
    /// 見つかりやすさ = いまの大きさ^conceal_size_power × (1 − 隠れの強さ × 足元の隠れ × (1 − シグナルで目立つ分))。
    /// 足元の隠れは、足元の草の割合と、巣の出来の良さ × nest_hide の大きい方。
    /// 見つけられる距離は体長（大きさの 1/3 乗）に比例するので、小さいほど見つかりにくい。草の隠れは大きさによらない。
    /// 相手の見える距離は 視野 × 見つかりやすさ になる。視野を超えず、どれだけ隠れても視野の 5% までは見える
    fn build_grid(&mut self) {
        let (exposure, hide, power) = (self.params.signal_exposure, self.params.cover_strength, self.params.conceal_size_power);
        let nest_hide = self.params.nest_hide;
        let reveal = self.params.motion_reveal;
        self.conceal.clear();
        self.motion.clear();
        for i in 0..self.count() {
            // 動いている相手ほど草に紛れない（前の tick に動いた量 ÷ 最高速度）
            let (vx, vy) = self.vel[i];
            let motion = ((vx * vx + vy * vy).sqrt() / self.traits[i].max_speed.max(1e-6)).min(1.0);
            self.motion.push(motion);
            let by_size = self.traits[i].size.max(0.01).powf(power);
            // 巣の中では、巣の隠れと足元の草の隠れの大きい方を使う
            let nest = self.nest[self.plants.cell_index(self.xs[i], self.ys[i])];
            let cover = self.cover[i].max(nest_hide * nest) * (1.0 - reveal * motion);
            let by_cover = 1.0 - hide * cover * (1.0 - exposure * self.signal[i]);
            let visible = (by_size * by_cover).clamp(0.05, 1.0);
            self.conceal.push(1.0 - visible);
        }
        self.grid.build(&self.xs, &self.ys, &self.kind, &self.conceal);
    }

    fn call_cell_of(&self, x: f32, y: f32) -> (usize, usize) {
        let c = ((x / self.params.call_cell) as usize).min(self.call_cols - 1);
        let r = ((y / self.params.call_cell) as usize).min(self.call_rows - 1);
        (c, r)
    }

    /// 呼び声の場を作り直す。成熟した個体の、直前の tick の繁殖シグナルを足し合わせる
    fn build_calls(&mut self) {
        for field in &mut self.calls {
            field.fill(0.0);
        }
        for i in 0..self.count() {
            if self.growth[i] >= 1.0 && self.signal[i] > 0.0 {
                let (c, r) = self.call_cell_of(self.xs[i], self.ys[i]);
                self.calls[self.kind[i] as usize][r * self.call_cols + c] += self.signal[i];
            }
        }
    }

    /// 個体 i に聞こえる、同じ種類の呼び声。戻り値は (世界の向きでの方向 dx, dy（単位ベクトルでない）, 周りの大きさの合計)。
    /// 周り 3 × 3 マスの呼び声を、マスの方向に重みを付けて足す。自分の声は除く
    fn hear_calls(&self, i: usize) -> (f32, f32, f32) {
        let field = &self.calls[self.kind[i] as usize];
        let (c, r) = self.call_cell_of(self.xs[i], self.ys[i]);
        let mine = if self.growth[i] >= 1.0 { self.signal[i] } else { 0.0 };
        let (mut dx, mut dy, mut total) = (0.0f32, 0.0f32, 0.0f32);
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                let rr = (r as i32 + dr).rem_euclid(self.call_rows as i32) as usize;
                let cc = (c as i32 + dc).rem_euclid(self.call_cols as i32) as usize;
                let mut v = field[rr * self.call_cols + cc];
                if dr == 0 && dc == 0 {
                    v = (v - mine).max(0.0);
                }
                total += v;
                dx += v * dc as f32;
                dy += v * dr as f32;
            }
        }
        (dx, dy, total)
    }

    /// 1 tick 進める
    pub fn step(&mut self) {
        self.step_deaths.clear();
        #[cfg(feature = "profile")]
        let mut laps = profile::Laps::start();
        // profile 機能のときだけ、前の区切りからの時間を段 k に足す
        macro_rules! lap {
            ($k:expr) => {
                #[cfg(feature = "profile")]
                laps.lap($k);
            };
        }
        // 季節で今の気温と植物の育ちやすさが変わる。変化はゆっくりなので、更新は CLIMATE_EVERY tick ごと
        if self.tick % CLIMATE_EVERY == 0 {
            self.update_climate();
        }
        let sunlight = self.params.sunlight * (1.0 + self.params.season_amplitude * self.season());
        self.plants.share = self.params.plant_shares();
        self.plants.woodland = self.params.woodland_contrast;
        self.ledger.sunlight_in += self.plants.grow(sunlight, self.params.plant_capacity);
        lap!(0);

        self.decay_carrion();
        self.decay_nests();
        let cap = self.params.plant_capacity;
        for i in 0..self.count() {
            self.cover[i] = self.plants.ratio_at(self.xs[i], self.ys[i], cap);
        }
        lap!(1);
        self.plants.update_landscape();
        lap!(2);
        self.build_grid();
        self.build_calls();
        self.prev_vel.clear();
        self.prev_vel.extend_from_slice(&self.vel);
        lap!(3);

        // 1 段目: 全個体が tick の始めの世界を見て、知覚と脳の判断をする（互いに書き込まないので並列にできる）。
        // セル順に並べる（近くの個体を続けて処理するとキャッシュに乗って速い）
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        order.extend((0..self.count()).map(|k| self.grid.entry(k).0));
        let mut plans = std::mem::take(&mut self.plans);
        self.make_plans(&order, &mut plans);
        self.noticed.clear();
        self.noticed.resize(self.count(), false);
        for (k, &i) in order.iter().enumerate() {
            self.noticed[i] = plans[k].input[genome::IN_PRED_NEAR] > 0.0;
        }
        lap!(4);
        // 2 段目: 判断に従って、1 体ずつ順番に行動する（移動・食べる・狩る・争う・繁殖）
        #[cfg(feature = "profile")]
        let mut part = [0.0f64; 3];
        // profile 機能のときだけ、式の時間を part[k] に足す
        macro_rules! timed {
            ($k:expr, $e:expr) => {{
                #[cfg(feature = "profile")]
                let t = std::time::Instant::now();
                let r = $e;
                #[cfg(feature = "profile")]
                {
                    part[$k] += t.elapsed().as_secs_f64();
                }
                r
            }};
        }
        for (k, &i) in order.iter().enumerate() {
            if !self.alive[i] {
                continue; // この tick ですでに食べられた
            }
            let eaten = self.life[i].eaten;
            let (speed, wants_child, mate) = timed!(0, self.act(i, &plans[k]));
            if !self.alive[i] {
                continue; // 行動の途中で死んだ（反撃や病気）
            }
            // 食べ物の場所の記憶: 草食は今いる場所の食べられる量 × 植生の合い具合、肉食はこの tick に食べた量
            let value = if self.kind[i] == HERBIVORE {
                let input = &plans[k].input;
                input[genome::IN_PLANT_HERE] * input[genome::IN_HABITAT_FIT]
            } else {
                ((self.life[i].eaten - eaten) / self.repro_threshold(i)).min(1.0)
            };
            self.remember(i, value);
            timed!(1, self.move_agent(i, speed));
            timed!(2, self.live(i, speed, wants_child, mate));
        }
        self.order = order;
        self.plans = plans;
        #[cfg(feature = "profile")]
        {
            for (k, t) in part.into_iter().enumerate() {
                profile::Laps::add(5 + k, t);
            }
            laps.reset();
        }

        self.apply_births();
        self.remove_dead();
        // 古い死亡の記録を捨てる
        while self.recent_deaths.front().is_some_and(|d| d.tick + DEATH_WINDOW <= self.tick) {
            self.recent_deaths.pop_front();
        }
        self.tick += 1;
        self.sample_history();
        lap!(8);
    }

    /// 視野の中で最も近い天敵・獲物・仲間を探す。
    /// 相手のいるマスの草が多いほど、その相手が見える距離は縮む（隠れ）
    fn sense(&self, i: usize) -> Senses {
        let (x, y) = (self.xs[i], self.ys[i]);
        let (w, h) = (self.width, self.height);
        let vision = self.traits[i].vision;
        let my_kind = self.kind[i];
        // [天敵, 獲物, 仲間]
        let mut best: [Seen; 3] = [None, None, None];
        let mut best_d2 = [f32::MAX; 3];
        let (mut kin_count, mut kin_hx, mut kin_hy, mut kin_alert) = (0u32, 0.0f32, 0.0f32, 0u32);
        let (mut kin_dx, mut kin_dy, mut alarm_hx, mut alarm_hy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let (mut sep_dx, mut sep_dy) = (0.0f32, 0.0f32);
        let mut kin_signal = 0.0f32;
        let mut rear_watchers = 0u32;
        let (mut rival, mut rival_d2, mut rival_count): (Seen, f32, u32) = (None, f32::MAX, 0);
        let mut prey_count = 0u32;
        // 近づきすぎない距離（体がぶつかる距離なので、草で隠れて見えなくても効く）
        let space = self.params.personal_space * self.traits[i].size;
        let space2 = space * space;
        // 草食は警戒しているほど天敵に遠くから気づける（警戒 0 で視野の vigilance_min_range 倍）。
        // 動いている天敵には、警戒していなくても動きの度合いだけ注意が向く（警戒と動きの大きい方）
        let vmin = self.params.vigilance_min_range;
        let vigilance = self.vigilance[i];
        let (mut similar, mut similar_d2, mut similar_count): (Seen, f32, u32) = (None, f32::MAX, 0);
        let (mut family, mut family_d2): (Seen, f32) = (None, f32::MAX);
        let (my_id, my_parents) = (self.id[i], self.parents[i]);
        let (mut child, mut child_d2): (Seen, f32) = (None, f32::MAX);
        // 成熟した仲間の最寄り（[標識色が近い, 問わない]）
        let mut mate: [Seen; 2] = [None, None];
        let mut mate_d2 = [f32::MAX; 2];
        let my_hue = self.traits[i].hue;
        let hue_similar = self.params.hue_similar;
        let vision2 = vision * vision;
        let (my_size, prey_min, prey_max) = (self.traits[i].size, self.traits[i].prey_min, self.traits[i].prey_max);
        let pref_softness = self.params.prey_pref_softness;
        // 視野角: 正面 (hx, hy) から見て、相手の方向との角度が視野角の半分以内なら見える。平方根を使わずに判定する
        let (my_hx, my_hy, fov_cos) = (self.hx[i], self.hy[i], self.traits[i].fov_cos);
        let in_view = |dx: f32, dy: f32, d2: f32| {
            let dot = dx * my_hx + dy * my_hy;
            let lim = d2 * fov_cos * fov_cos;
            if fov_cos >= 0.0 { dot >= 0.0 && dot * dot >= lim } else { dot >= 0.0 || dot * dot <= lim }
        };
        // 相手の種類と足元の草はグリッドがセル順に並べた写しから受け取る（離れたメモリを読まずに済む）。
        // 安い判定から順に行い、遠い相手はそこで打ち切る
        self.grid.for_each_within(x, y, vision, |j, xj, yj, kind_j, cover_j| {
            let dx = torus_delta(xj - x, w);
            let dy = torus_delta(yj - y, h);
            let d2 = dx * dx + dy * dy;
            if d2 >= vision2 {
                return;
            }
            if kind_j == my_kind && d2 < space2 && d2 > 1e-6 && j != i {
                let d = d2.sqrt();
                let push = 1.0 - d / space;
                sep_dx -= dx / d * push;
                sep_dy -= dy / d * push;
            }
            // 自分の幼い子（初期個体の親の id は 0 なので、my_id が 0 でなければ当てはまらない）
            if kind_j == my_kind && j != i && d2 < child_d2 && self.growth[j] < 1.0 && self.alive[j] && self.parents[j].contains(&my_id) {
                child_d2 = d2;
                child = Some((j, dx, dy, d2));
            }
            // 相手の見えにくさ（隠れの強さ・足元の草・シグナル・大きさを build_grid で掛け合わせたもの）
            let mut r = vision * (1.0 - cover_j);
            if my_kind == HERBIVORE && kind_j != my_kind {
                let attention = vigilance.max(self.motion.get(j).copied().unwrap_or(0.0));
                r *= vmin + (1.0 - vmin) * attention;
            }
            if d2 < r * r && j != i && self.alive[j] && in_view(dx, dy, d2) {
                let rel = if kind_j == my_kind {
                    2
                } else if my_kind == HERBIVORE {
                    0
                } else {
                    1
                };
                // 肉食は、好みの大きさの範囲から外れた獲物ほど、獲物として気づく距離が縮む（目の前なら狙う）。
                // どの獲物を狙うかも、好みで割り引いた距離（距離 ÷ 好み）が最も近いものにする
                // （好みの大きさの獲物が少し遠くにいれば、目の前の小さすぎる獲物より優先する）
                let (wanted, rank) = if rel == 1 {
                    let ratio = self.traits[j].size / my_size;
                    let pref = prey_preference(ratio, prey_min, prey_max, pref_softness);
                    let reach = r * pref;
                    (d2 < reach * reach, d2 / (pref * pref).max(1e-6))
                } else {
                    (true, d2)
                };
                prey_count += (rel == 1 && wanted) as u32;
                if wanted && rank < best_d2[rel] {
                    best_d2[rel] = rank;
                    best[rel] = Some((j, dx, dy, d2));
                }
                if rel == 2 {
                    kin_count += 1;
                    kin_hx += self.hx[j];
                    kin_hy += self.hy[j];
                    // 草食なら天敵が、肉食なら獲物が見えている仲間
                    let s = self.state[j];
                    let alert = s == state::FLEE || s == state::CHASE;
                    kin_alert += alert as u32;
                    kin_signal += self.signal[j];
                    // 仲間 j が自分の真後ろの方向（−自分の向き）を見ているか。近くにいるので位置の差は無視し、向きだけで判定する
                    let back = -(self.hx[j] * my_hx + self.hy[j] * my_hy);
                    rear_watchers += (back >= self.traits[j].fov_cos) as u32;
                    kin_dx += dx;
                    kin_dy += dy;
                    if alert {
                        alarm_hx += self.hx[j];
                        alarm_hy += self.hy[j];
                    }
                    let is_similar = genome::hue_distance(my_hue, self.traits[j].hue) <= hue_similar;
                    if is_similar {
                        similar_count += 1;
                        if d2 < similar_d2 {
                            similar_d2 = d2;
                            similar = Some((j, dx, dy, d2));
                        }
                    } else {
                        rival_count += 1;
                        if d2 < rival_d2 {
                            rival_d2 = d2;
                            rival = Some((j, dx, dy, d2));
                        }
                    }
                    // 親子: j が自分の親か、自分が j の親（初期個体の親の id は 0 なので当てはまらない）
                    let pj = self.parents[j];
                    let is_family = pj[0] == my_id || pj[1] == my_id || (self.id[j] != 0 && my_parents.contains(&self.id[j]));
                    if is_family && d2 < family_d2 {
                        family_d2 = d2;
                        family = Some((j, dx, dy, d2));
                    }
                    if self.growth[j] >= 1.0 {
                        for (k, ok) in [is_similar, true].into_iter().enumerate() {
                            if ok && d2 < mate_d2[k] {
                                mate_d2[k] = d2;
                                mate[k] = Some((j, dx, dy, d2));
                            }
                        }
                    }
                }
            }
        });
        Senses {
            predator: best[0],
            prey: best[1],
            kin: best[2],
            kin_count,
            kin_hx,
            kin_hy,
            kin_alert,
            similar,
            similar_count,
            mate: mate[0].or(mate[1]),
            sep_dx,
            sep_dy,
            kin_dx,
            kin_dy,
            alarm_hx,
            alarm_hy,
            kin_signal,
            rear_watchers,
            rival,
            rival_count,
            prey_count,
            family,
            child,
        }
    }

    /// 知覚 → 脳 → 行動。戻り値は (この tick の移動速度, 繁殖したいか, 繁殖相手)
    /// 全個体の判断を作る。parallel 機能が有効で use_parallel なら並列に、そうでなければ順番に計算する（結果は同じ）
    fn make_plans(&self, order: &[usize], plans: &mut Vec<Plan>) {
        #[cfg(feature = "parallel")]
        if self.use_parallel {
            use rayon::prelude::*;
            order.par_iter().map(|&i| self.plan(i)).collect_into_vec(plans);
            return;
        }
        plans.clear();
        plans.extend(order.iter().map(|&i| self.plan(i)));
    }

    /// 知覚 → 脳の判断。世界には書き込まない
    fn plan(&self, i: usize) -> Plan {
        #[cfg(feature = "profile")]
        let mut sw = profile::Stopwatch::start();
        // profile 機能のときだけ、前の区切りからの時間を判断の段の区切り k に足す
        macro_rules! lap {
            ($k:expr) => {
                #[cfg(feature = "profile")]
                sw.lap($k);
            };
        }
        let senses = self.sense(i);
        lap!(0);
        let t = self.traits[i];
        let (x, y) = (self.xs[i], self.ys[i]);
        let (hx, hy) = (self.hx[i], self.hy[i]);
        // 世界の向き (dx, dy) を、自分の向きを基準にした (前方, 右方) に直す。右方は (−hy, hx)
        let to_local = |dx: f32, dy: f32| (dx * hx + dy * hy, -dx * hy + dy * hx);

        let mut input = [0.0f32; N_IN];
        input[genome::IN_ENERGY] = self.energy[i] / self.repro_threshold(i);
        // 足元の草: 草食は自分が食べられる量（届く層の量 × 質から食べ残しを引いたもの）、肉食は草の量の合計（隠れ場所の手がかり）
        input[genome::IN_PLANT_HERE] = if self.kind[i] == HERBIVORE {
            let c = self.plants.cell_index(x, y);
            (self.food_value(&self.reach(i), self.graze_floor(i), c) / self.params.plant_capacity).min(1.0)
        } else {
            self.cover[i]
        };
        let fit_here = self.habitat_fit(i, self.terrain.soil_at(x, y));
        input[genome::IN_HABITAT_FIT] = fit_here;
        input[genome::IN_TEMP_DIFF] = (self.terrain.temperature_at(x, y) - t.temp_pref) / 10.0;
        // 草食は、自分の食べられる植生の草を優先して探す
        let gradient = if self.kind[i] == HERBIVORE {
            let (natal, p) = (self.natal[i], &self.params);
            let (tol, lim) = (p.habitat_tolerance, p.habitat_limit);
            let (reach, floor) = (self.reach(i), self.graze_floor(i));
            self.plants.richer_direction_by(x, y, |k| {
                let f = self.plants.vegetation[k];
                let t = (((f - natal).abs() - tol) / (lim - tol).max(1e-6)).clamp(0.0, 1.0);
                self.food_value(&reach, floor, k) * (1.0 - t * t * (3.0 - 2.0 * t))
            })
        } else {
            None
        };
        if let Some((dc, dr)) = gradient {
            let len = (dc * dc + dr * dr).sqrt();
            let (f, r) = to_local(dc / len, dr / len);
            input[genome::IN_GRAD_FWD] = f;
            input[genome::IN_GRAD_RIGHT] = r;
        }
        // 遠くの景色: 見渡せる距離に、食べ物の多い場所があればその方向
        if self.kind[i] == HERBIVORE {
            if let Some((dx, dy)) = self.far_food_direction(i, &self.reach(i), self.graze_floor(i)) {
                let (f, r) = to_local(dx, dy);
                input[genome::IN_FAR_FWD] = f;
                input[genome::IN_FAR_RIGHT] = r;
            }
        }
        lap!(1);
        for (seen, at) in [
            (senses.predator, genome::IN_PRED_FWD),
            (senses.prey, genome::IN_PREY_FWD),
            (senses.kin, genome::IN_KIN_FWD),
            (senses.family, genome::IN_FAMILY_FWD),
            (senses.child, genome::IN_CHILD_FWD),
        ] {
            if let Some((_, dx, dy, d2)) = seen {
                let d = d2.sqrt().max(1e-3);
                let (f, r) = to_local(dx / d, dy / d);
                input[at] = f;
                input[at + 1] = r;
                // 見えていれば 0.5〜1（近いほど大きい）、見えていなければ 0。
                // 「見えたか」と「どれだけ近いか」の両方をこの 1 つで伝える
                input[at + 2] = 1.0 - 0.5 * (d / t.vision).min(1.0);
            }
        }

        if senses.kin_count > 0 {
            let n = senses.kin_count as f32;
            input[genome::IN_KIN_COUNT] = softsign(n / 4.0);
            let (f, r) = to_local(senses.kin_hx / n, senses.kin_hy / n);
            input[genome::IN_KIN_HEAD_FWD] = f;
            input[genome::IN_KIN_HEAD_RIGHT] = r;
            input[genome::IN_KIN_ALERT] = senses.kin_alert as f32 / n;
            input[genome::IN_SIM_RATIO] = senses.similar_count as f32 / n;
            input[genome::IN_KIN_SIGNAL] = senses.kin_signal / n;
            input[genome::IN_REAR_WATCH] = senses.rear_watchers as f32 / n;
        }
        input[genome::IN_SEASON] = self.season();
        if let Some((j, dx, dy, d2)) = senses.rival {
            let d = d2.sqrt().max(1e-3);
            let (f, r) = to_local(dx / d, dy / d);
            input[genome::IN_RIVAL_FWD] = f;
            input[genome::IN_RIVAL_RIGHT] = r;
            input[genome::IN_RIVAL_NEAR] = 1.0 - 0.5 * (d / t.vision).min(1.0);
            input[genome::IN_RIVAL_SIZE] = self.traits[j].size / t.size;
        }
        input[genome::IN_RIVAL_COUNT] = softsign(senses.rival_count as f32 / 4.0);
        input[genome::IN_ATTACKED] = self.attacked[i] as f32 / ATTACKED_TICKS as f32;
        // 仲間が見えていなければ一人の時間が延びる（書き込みは行動の段階で行う）
        let alone = if senses.kin_count > 0 { 0 } else { self.alone[i].saturating_add(1) };
        input[genome::IN_ALONE] = softsign(alone as f32 / 150.0);
        // 周りの食べ物の乏しさ: 仲間が多く、食べ物が少ないほど 1 に近い
        if senses.kin_count > 0 {
            let food = if self.kind[i] == HERBIVORE { self.plants_around(i) } else { senses.prey_count as f32 };
            let n = senses.kin_count as f32;
            input[genome::IN_SCARCITY] = n / (n + 4.0 * food);
        }
        if self.kind[i] == CARNIVORE {
            let c = self.plants.cell_index(x, y);
            input[genome::IN_CARRION_HERE] = softsign(self.carrion[c] / 2.0);
            if self.carrion[c] > 0.0 {
                input[genome::IN_CARRION_FRESH] = self.carrion_freshness(c);
            }
            if let Some((dc, dr)) = self.carrion_direction(x, y) {
                let len = (dc * dc + dr * dr).sqrt();
                let (f, r) = to_local(dc / len, dr / len);
                input[genome::IN_CARRION_FWD] = f;
                input[genome::IN_CARRION_RIGHT] = r;
            }
        }
        let (call_dx, call_dy, call_total) = self.hear_calls(i);
        let call_dir = {
            let (f, r) = to_local(call_dx, call_dy);
            let len = (f * f + r * r).sqrt();
            if len < 1e-6 { (0.0, 0.0) } else { (f / len, r / len) }
        };
        input[genome::IN_CALL_FWD] = call_dir.0;
        input[genome::IN_CALL_RIGHT] = call_dir.1;
        input[genome::IN_CALL_LEVEL] = softsign(call_total / 2.0);
        if let Some((_, dx, dy, d2)) = senses.similar {
            let d = d2.sqrt().max(1e-3);
            let (f, r) = to_local(dx / d, dy / d);
            input[genome::IN_SIM_FWD] = f;
            input[genome::IN_SIM_RIGHT] = r;
        }

        let my_size = t.size;
        if let Some((j, _, _, _)) = senses.prey {
            input[genome::IN_PREY_SIZE] = self.traits[j].size / my_size;
        }
        if let Some((j, _, _, _)) = senses.predator {
            input[genome::IN_PRED_SIZE] = self.traits[j].size / my_size;
        }
        let ahead_x = wrap(x + hx * genome::WATER_LOOKAHEAD, self.width);
        let ahead_y = wrap(y + hy * genome::WATER_LOOKAHEAD, self.height);
        input[genome::IN_WATER_AHEAD] = self.terrain.is_water(ahead_x, ahead_y) as u8 as f32;
        let ready = self.can_breed(i);
        input[genome::IN_READY] = ready as u8 as f32;
        if let Some((_, _, _, d2)) = senses.mate {
            input[genome::IN_MATE_NEAR] = 1.0 - 0.5 * (d2.sqrt() / t.vision).min(1.0);
        }

        // 覚えている食べ物の場所: 良さは記憶の長さで薄れる。その場所にいれば方向は 0
        let mem = self.memory[i];
        let recalled = self.recalled_value(i);
        if recalled > 0.0 {
            input[genome::IN_MEM_VALUE] = recalled;
            let dx = torus_delta(mem.x - x, self.width);
            let dy = torus_delta(mem.y - y, self.height);
            let d = (dx * dx + dy * dy).sqrt();
            if d > self.plants.cell_size {
                let (f, r) = to_local(dx / d, dy / d);
                input[genome::IN_MEM_FWD] = f;
                input[genome::IN_MEM_RIGHT] = r;
            }
        }

        // 巣: 今いるマスの出来の良さと、覚えている巣の方向（そのマスにいれば 0）
        let here = self.plants.cell_index(x, y);
        input[genome::IN_NEST_HERE] = self.nest[here];
        let home = self.home[i];
        if home != NO_HOME && home as usize != here {
            let cs = self.plants.cell_size;
            let (hc, hr) = (home as usize % self.plants.cols, home as usize / self.plants.cols);
            let dx = torus_delta((hc as f32 + 0.5) * cs - x, self.width);
            let dy = torus_delta((hr as f32 + 0.5) * cs - y, self.height);
            let d = (dx * dx + dy * dy).sqrt().max(1e-3);
            let (f, r) = to_local(dx / d, dy / d);
            input[genome::IN_NEST_FWD] = f;
            input[genome::IN_NEST_RIGHT] = r;
            input[genome::IN_NEST_AWAY] = 1.0;
        }
        input[genome::IN_STAMINA] = self.stamina[i];
        input[genome::IN_GROWTH] = self.growth[i];
        input[genome::IN_GESTATION] = self.gest_energy[i] / self.repro_threshold(i);
        if let Some((j, ..)) = senses.child {
            input[genome::IN_CHILD_HUNGER] = (1.0 - self.energy[j] / self.energy_cap(j).max(1e-6)).clamp(0.0, 1.0);
        }
        // 最後に子を産んだ場所（その場所にいれば 0）
        if self.tick < self.den_until[i] {
            let dx = torus_delta(self.den[i].0 - x, self.width);
            let dy = torus_delta(self.den[i].1 - y, self.height);
            let d = (dx * dx + dy * dy).sqrt();
            if d > 1.0 {
                let (f, r) = to_local(dx / d, dy / d);
                input[genome::IN_DEN_FWD] = f;
                input[genome::IN_DEN_RIGHT] = r;
            }
        }

        lap!(2);
        let out = genome::think(self.genome(i), &input);
        lap!(3);
        Plan { senses, input, out, call_dir, alone }
    }

    /// 判断 plan に従って行動する。戻り値は (この tick の移動速度, 繁殖したいか, 繁殖相手)
    fn act(&mut self, i: usize, plan: &Plan) -> (f32, bool, Mate) {
        let Plan { senses, input, out, call_dir, alone } = *plan;
        self.alone[i] = alone;
        let t = self.traits[i];
        let (hx, hy) = (self.hx[i], self.hy[i]);
        let to_local = |dx: f32, dy: f32| (dx * hx + dy * hy, -dx * hy + dy * hx);

        // 欲求ごとの方向（自分基準の単位ベクトル。対象がいなければ (0, 0)）
        let unit = |seen: Seen| match seen {
            Some((_, dx, dy, d2)) => {
                let d = d2.sqrt().max(1e-3);
                to_local(dx / d, dy / d)
            }
            None => (0.0, 0.0),
        };
        let normalize = |(f, r): (f32, f32)| {
            let len = (f * f + r * r).sqrt();
            if len < 1e-6 { (0.0, 0.0) } else { (f / len, r / len) }
        };
        // 食べ物の方向: 草食は自分の食べられる草の多い方へ。肉食は獲物へ。
        // 肉食は獲物が見えなくても、獲物を追っている仲間がいればその向きへ（群れの目）
        // 草食は、隣のマスに草の多い方向がなければ、遠くの景色で見つけた場所へ
        let food = if self.kind[i] == HERBIVORE {
            if input[genome::IN_GRAD_FWD] != 0.0 || input[genome::IN_GRAD_RIGHT] != 0.0 {
                (input[genome::IN_GRAD_FWD], input[genome::IN_GRAD_RIGHT])
            } else {
                (input[genome::IN_FAR_FWD], input[genome::IN_FAR_RIGHT])
            }
        } else if senses.prey.is_some() {
            unit(senses.prey)
        } else if input[genome::IN_CARRION_FWD] != 0.0 || input[genome::IN_CARRION_RIGHT] != 0.0 {
            // 獲物が見えなければ、近くの死体へ
            (input[genome::IN_CARRION_FWD], input[genome::IN_CARRION_RIGHT])
        } else {
            normalize(to_local(senses.alarm_hx, senses.alarm_hy))
        };
        // 逃げる方向: 天敵が見えていればその反対。見えていなくても、逃げている仲間がいればその向き（警戒の合図）
        // 逃げる方向: 天敵が見えていればその反対。敵に攻撃されて負けた直後なら、最寄りの敵の反対。
        // どちらでもなく、逃げている仲間がいればその向き（警戒の合図）
        let flee = match (senses.predator, senses.rival) {
            (Some(_), _) => {
                let (pf, pr) = unit(senses.predator);
                (-pf, -pr)
            }
            (None, Some(_)) if self.attacked[i] > 0 => {
                let (rf, rr) = unit(senses.rival);
                (-rf, -rr)
            }
            _ => normalize(to_local(senses.alarm_hx, senses.alarm_hy)),
        };
        // 仲間に寄る方向: 見えている仲間の中心へ
        let kin = normalize(to_local(senses.kin_dx, senses.kin_dy));
        // 繁殖相手へ向かう方向: 見えていればその相手へ。見えなければ、呼び声の大きい方へ
        let mate_dir = if senses.mate.is_some() { unit(senses.mate) } else { call_dir };
        // 追い払う方向: 最寄りの敵へ
        let drives = [food, mate_dir, flee, kin, unit(senses.rival), (0.0, 0.0)];
        if self.kind[i] == HERBIVORE {
            self.vigilance[i] = genome::vigilance(&out);
        }
        self.signal[i] = genome::signal(&out);
        let (mut df, mut dr) = genome::mix_drives(&out, &drives);
        // 近づきすぎた個体からは離れる（欲求の優先度とは別の、固定のルール）
        let (sf, sr) = to_local(senses.sep_dx, senses.sep_dy);
        df += self.params.separation_weight * sf;
        dr += self.params.separation_weight * sr;
        let dominant = self.dominant_drive(&out, &drives);

        let is_watched = self.id[i] == self.watch.id;
        if is_watched {
            self.watch.output = out;
            self.watch.input.clear();
            self.watch.input.extend_from_slice(&input);
            self.watch.priorities = genome::priorities(&out);
        }

        // 疲れていると（体力が STAMINA_FADE を切ると）、出せる速さの上限が巡航の速さまで下がっていく
        let throttle = (0.5 + 0.5 * softsign(out[genome::OUT_THROTTLE])).min(self.stamina_cap(i));
        // 幼体は遅い。飢えた個体と老いた個体も遅い
        let jv = self.params.juvenile_speed;
        let vigor = self.vigor(i);
        let speed = t.max_speed * throttle * vigor * (jv + (1.0 - jv) * self.growth[i]) * self.mobility(i)
            / (1.0 + self.gestation_load(i));

        // 行きたい方向へ曲がる。1 tick に曲がれる角度には上限がある
        let (turn_cos, turn_sin) = self.turn_limit(i, speed);
        let (lf, lr) = steer(df, dr, turn_cos, turn_sin);
        // 曲がるとエネルギーを使う（曲がった角度は 0.5 ラジアン程度までなので、sin で近似する）
        let turn_energy = (self.params.turn_cost * lr.abs() * t.size).min(self.energy[i]);
        self.energy[i] -= turn_energy;
        self.ledger.lost_metabolism += turn_energy as f64;
        // 自分基準の (前方, 右方) を世界の向きに戻し、小さな揺れを加える
        let (mut nx, mut ny) = (lf * hx - lr * hy, lf * hy + lr * hx);
        let noise = self.rng.range(-self.params.turn_noise, self.params.turn_noise);
        // 小さな角度 ε の回転は (x − εy, y + εx) で近似できる。ずれた長さは直後に 1 に戻す
        (nx, ny) = (nx - noise * ny, ny + noise * nx);
        let len = (nx * nx + ny * ny).sqrt();
        self.hx[i] = nx / len;
        self.hy[i] = ny / len;
        if is_watched {
            self.watch.signal = self.signal[i];
            self.watch.vigor = vigor;
            self.watch.turn = lr.atan2(lf);
            self.watch.throttle = throttle;
        }
        let wants_eat = out[genome::OUT_EAT] > 0.0;
        let wants_child = out[genome::OUT_REPRODUCE] > 0.0;
        let mate = self.find_mate(i, &senses);
        if is_watched {
            self.watch.mate_nearby = !matches!(mate, Mate::Missing);
            self.watch.mate_compat = match mate {
                Mate::Partner(j) => self.mating_chance(i, j),
                Mate::Asexual => 1.0,
                Mate::Missing => -1.0,
            };
        }

        // 届く距離の敵を攻撃する（攻撃した tick は、食べたり狩ったりしない）
        if out[genome::OUT_ATTACK] > 0.0
            && self.cooldown[i] == 0
            && let Some((j, _, _, d2)) = senses.rival
            && self.alive[j]
        {
            let reach = self.params.fight_range * t.size;
            if d2 < reach * reach {
                self.census.fights[self.kind[i] as usize] += 1;
                self.fight(i, j, death::FOUGHT);
                self.state[i] = state::FIGHT;
                return (speed, wants_child, mate);
            }
        }

        // 届く距離の仲間（同じ系統）を攻撃する
        if out[genome::OUT_ATTACK_KIN] > 0.0
            && self.cooldown[i] == 0
            && let Some((j, _, _, d2)) = senses.similar
            && self.alive[j]
        {
            let reach = self.params.fight_range * t.size;
            if d2 < reach * reach {
                self.census.kin_fights[self.kind[i] as usize] += 1;
                self.fight(i, j, death::KIN_FOUGHT);
                self.state[i] = state::FIGHT_KIN;
                return (speed, wants_child, mate);
            }
        }

        // 届く距離にいる自分の幼い子に与える（与えた tick は動かず、食べない）
        if out[genome::OUT_FEED] > 0.0
            && let Some((j, _, _, d2)) = senses.child
            && self.alive[j]
        {
            let reach = self.params.feed_range * t.size;
            if d2 < reach * reach && self.feed(i, j) {
                self.state[i] = state::FEED;
                return (0.0, wants_child, mate);
            }
        }

        let wants_build = out[genome::OUT_BUILD] > 0.0;
        if self.kind[i] == HERBIVORE {
            let ate = wants_eat && self.graze(i);
            // 食べなかった tick に、作りたければ今いるマスで巣を作る（作る tick は動かない）
            if !ate && wants_build && self.build_nest(i) {
                self.state[i] = state::BUILD;
                return (0.0, wants_child, mate);
            }
            self.state[i] = if senses.predator.is_some() {
                state::FLEE
            } else if dominant == DRIVE_MATE {
                state::COURT
            } else if ate {
                state::GRAZE
            } else {
                state::SEEK
            };
            (speed, wants_child, mate)
        } else {
            // 判断のあとで、同じ tick にほかの肉食が先に仕留めていることがある
            if wants_eat && let Some((j, _, _, d2)) = senses.prey
                && self.alive[j]
                && self.mobility(i) > 0.0
            {
                match self.try_hunt(i, j, d2) {
                    Hunt::Caught => {
                        self.state[i] = state::EAT;
                        return (0.0, wants_child, mate); // 食べた tick は動かない
                    }
                    Hunt::Failed => {
                        self.state[i] = state::MISSED;
                        return (speed, wants_child, mate);
                    }
                    Hunt::NoAttempt => {}
                }
            }
            // 狩らなかったら、足元の死体を食べる（食べる tick は動かない）
            if wants_eat && self.scavenge(i) {
                self.state[i] = state::SCAVENGE;
                return (0.0, wants_child, mate);
            }
            // 何も食べなかった tick に、作りたければ今いるマスで巣を作る（作る tick は動かない）
            if wants_build && self.build_nest(i) {
                self.state[i] = state::BUILD;
                return (0.0, wants_child, mate);
            }
            self.state[i] = if dominant == DRIVE_MATE {
                state::COURT
            } else if senses.prey.is_some() {
                state::CHASE
            } else {
                state::WANDER
            };
            (speed, wants_child, mate)
        }
    }

    /// 速さ speed で走っている個体 i が 1 tick に曲がれる最大の角度の (cos, sin)。
    /// 大きいほど小回りが利かず、横向きに出せる力には限りがあるので、速く走っているほど曲がれない
    pub fn turn_limit(&self, i: usize, speed: f32) -> (f32, f32) {
        let t = &self.traits[i];
        let (max_turn, accel) = (self.params.max_turn, self.params.turn_accel);
        if speed * max_turn <= accel {
            return (t.turn_cos, t.turn_sin); // 体の大きさで決まる上限のまま（成長に合わせて計算済み）
        }
        let (sin, cos) = (accel / speed / t.size.sqrt()).min(std::f32::consts::PI).sin_cos();
        (cos, sin)
    }

    /// いちばん効いている欲求（優先度 × 方向の長さ が最大のもの）。観察用
    fn dominant_drive(&self, out: &[f32; genome::N_OUT], drives: &[(f32, f32); genome::N_DRIVES]) -> usize {
        let p = genome::priorities(out);
        let free = (softsign(out[genome::OUT_DIR_FWD]), softsign(out[genome::OUT_DIR_RIGHT]));
        let mut best = (0, -1.0f32);
        for k in 0..genome::N_DRIVES {
            let (f, r) = if k == genome::N_DRIVES - 1 { free } else { drives[k] };
            let strength = p[k] * (f * f + r * r).sqrt();
            if strength > best.1 {
                best = (k, strength);
            }
        }
        best.0
    }

    /// 繁殖相手を決める。繁殖相手を探せる距離以内にいる、繁殖相手の候補（成熟した仲間で、
    /// 標識色が近い個体を優先）が相手になる。その距離が 0 以下なら無性生殖（相手は要らない）
    fn find_mate(&self, i: usize, senses: &Senses) -> Mate {
        let range = if self.kind[i] == HERBIVORE { self.params.herb_mate_range } else { self.params.carn_mate_range };
        if range <= 0.0 {
            return Mate::Asexual;
        }
        match senses.mate {
            Some((j, _, _, d2)) if d2 <= range * range && self.alive[j] => Mate::Partner(j),
            _ => Mate::Missing,
        }
    }

    /// いまいるマスの草を食べる。蓄えの上限を超える分は食べない。食べたら true
    /// 草食が食べた植物のうちエネルギーになる割合。大きい体ほど草を長く腸にとどめて多くを取り出せる
    /// （ジャーマン・ベルの原理）ので、大人の大きさの herb_digest_size_power 乗に比例させる
    pub fn digest_efficiency(&self, i: usize) -> f32 {
        let p = &self.params;
        (p.herb_efficiency * self.traits[i].adult_size.max(0.01).powf(p.herb_digest_size_power)).min(0.95)
    }

    fn graze(&mut self, i: usize) -> bool {
        let room = self.energy_cap(i) - self.energy[i];
        if room <= 0.0 {
            return false;
        }
        let eff = self.digest_efficiency(i);
        let c = self.plants.cell_index(self.xs[i], self.ys[i]);
        // 一口の大きさ: 警戒しているほど小さく（周りを見ながらでは食べる量が減る）、
        // 生まれた場所と違う植生の草ほど小さい（うまく食べられない）
        let fit = self.habitat_fit(i, self.plants.vegetation[c]);
        let bite = self.params.herb_bite
            * self.traits[i].size
            * (1.0 - self.params.vigilance_bite_cost * self.vigilance[i])
            * fit
            * self.mobility(i);
        // 届く層のうち、取れる量 × 質 が最も大きい層から食べる。硬い層ほど身になる割合が低い。
        // 地面の草は、口の大きさで決まる食べ残す高さより上しか取れない
        let (reach, floor) = (self.reach(i), self.graze_floor(i));
        let quality = self.params.plant_qualities();
        let (mut layer, mut best) = (0, -1.0);
        for k in 0..LAYERS {
            let v = quality[k] * self.layer_available(&reach, floor, k, c);
            if v > best {
                (layer, best) = (k, v);
            }
        }
        let available = self.layer_available(&reach, floor, layer, c);
        let eff = eff * quality[layer];
        let take = bite.min(available).min(room / eff.max(1e-6));
        let b = herb_size_bin(self.traits[i].adult_size);
        let cs = &mut self.census;
        cs.herb_size_grazes[b] += 1;
        cs.herb_size_graze_short[b] += (available < bite.min(room / eff.max(1e-6))) as u64;
        cs.herb_size_bite[b] += bite as f64;
        cs.herb_size_take[b] += take.max(0.0) as f64;
        if take <= 0.0 {
            return false;
        }
        self.plants.take(c, layer, take);
        let gain = take * eff;
        self.energy[i] += gain;
        self.ledger.herb_gain += gain as f64;
        self.life[i].eaten += gain;
        self.ledger.lost_digestion += (take - gain) as f64;
        true
    }

    /// 獲物 j を襲う。届く距離にいて、大きすぎなければ食べる。食べたら true
    fn try_hunt(&mut self, i: usize, j: usize, d2: f32) -> Hunt {
        let range = self.params.carn_attack_range * self.traits[i].size;
        if d2 >= range * range || self.cooldown[i] > 0 {
            return Hunt::NoAttempt;
        }
        // 襲いかかる。成功・失敗にかかわらず、大きさに比例したエネルギーを使い、しばらく次を襲えない
        let cost = (self.params.attack_cost * self.traits[i].size).min(self.energy[i]);
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.cooldown[i] = self.params.attack_cooldown as u16;
        let odds = self.hunt_odds(i, j);
        self.life[i].hunts += 1;
        let c = &mut self.census;
        c.hunts += 1;
        c.hunt_ratio += odds.size_ratio as f64;
        c.hunt_guards += odds.guards as f64;
        c.hunt_allies += odds.allies as f64;
        c.hunt_rivals += odds.rivals as f64;
        c.hunt_chance += odds.chance as f64;
        c.hunt_escape += odds.escape as f64;
        let noticed = self.noticed.get(j).copied().unwrap_or(false);
        c.hunt_noticed += noticed as u64;
        if self.kind[j] == HERBIVORE {
            c.herb_size_attacked[herb_size_bin(self.traits[j].adult_size)] += 1;
        }
        let cb = carn_size_bin(self.traits[i].adult_size);
        c.carn_size_hunts[cb] += 1;
        c.carn_size_ratio[cb] += odds.size_ratio as f64;
        c.carn_size_attack_cost[cb] += cost as f64;
        if self.rng.f32() >= odds.chance {
            self.counterattack(i, &odds);
            return Hunt::Failed; // 逃げられた
        }
        // 仕留めた獲物は、自分と近くの仲間（同じ系統の肉食）で等分する。
        // 各自の蓄えの上限を超える分と、吸収できない分は熱として失われる
        // 獲物の価値は、蓄えと体の両方
        let prey = self.energy[j] + self.body_energy(j);
        let (_, mut eaters) = self.carnivore_neighbors(i);
        // 動けない子は獲物のところへ来られない（親が与える）
        eaters.retain(|&k| self.mobility(k) > 0.0);
        eaters.push(i);
        self.census.kills += 1;
        self.census.kill_noticed += noticed as u64;
        self.census.carn_size_kills[carn_size_bin(self.traits[i].adult_size)] += 1;
        self.life[i].kills += 1;
        self.census.kill_eaters += eaters.len() as f64;
        let share = prey * self.params.carn_efficiency / eaters.len() as f32;
        let mut gained = 0.0;
        for &k in &eaters {
            let room = (self.energy_cap(k) - self.energy[k]).max(0.0);
            let gain = share.min(room);
            self.energy[k] += gain;
            self.life[k].eaten += gain;
            self.census.carn_size_gain[carn_size_bin(self.traits[k].adult_size)] += gain as f64;
            gained += gain;
        }
        self.ledger.carn_gain += gained as f64;
        // 食べきれなかった分（蓄えが満杯で食べられなかった分）は死体として残る。吸収できない分は熱として失われる
        let eaten = gained / self.params.carn_efficiency;
        self.ledger.lost_digestion += (eaten - gained) as f64;
        let (jx, jy) = (self.xs[j], self.ys[j]);
        self.drop_carrion(jx, jy, (prey - eaten).max(0.0));
        self.energy[j] = 0.0;
        self.die(j, death::EATEN);
        Hunt::Caught
    }

    /// 肉食 i の近く（coop_radius 以内）にいる、生きている肉食を、仲間と系統の違う相手に分ける。
    /// 仲間は標識色が近い（同じ系統の）肉食。戻り値は (系統の違う相手の数, 仲間の番号)
    fn carnivore_neighbors(&self, i: usize) -> (u32, Vec<usize>) {
        let p = &self.params;
        let (x, y) = (self.xs[i], self.ys[i]);
        let r2 = p.coop_radius * p.coop_radius;
        let my_hue = self.traits[i].hue;
        let mut rivals = 0u32;
        let mut allies = Vec::new();
        self.grid.for_each_within(x, y, p.coop_radius, |k, xk, yk, kind_k, _| {
            if k == i || kind_k != CARNIVORE || !self.alive[k] {
                return;
            }
            let dx = torus_delta(xk - x, self.width);
            let dy = torus_delta(yk - y, self.height);
            if dx * dx + dy * dy >= r2 {
                return;
            }
            if genome::hue_distance(my_hue, self.traits[k].hue) <= p.hue_similar {
                allies.push(k);
            } else {
                rivals += 1;
            }
        });
        (rivals, allies)
    }

    /// 肉食 i が獲物 j を襲って成功する確率。
    /// 獲物が自分より大きいほど下がり、獲物の近くに仲間（同じ種類）が多いほど下がる（数で守る）。
    /// 近くの仲間（同じ系統の肉食）が多いほど上がり、大きな獲物も狩れる（協力）。
    /// 近くに系統の違う肉食が多いほど下がる（干渉）
    pub fn hunt_chance(&self, i: usize, j: usize) -> f32 {
        self.hunt_odds(i, j).chance
    }

    /// 狩りに失敗したとき、獲物の反撃でけがをする。獲物が大きいほど、獲物の近くに仲間が多いほど痛い。
    /// けがで失うエネルギーが手持ちを超えたら死ぬ
    fn counterattack(&mut self, i: usize, odds: &HuntOdds) {
        let p = &self.params;
        let excess = odds.size_ratio - p.counter_start_ratio;
        if excess <= 0.0 {
            return;
        }
        let damage = p.counter_damage * excess * (1.0 + p.guard_strength * odds.guards as f32);
        if damage >= self.energy[i] {
            // けがで死ぬ。残っていたエネルギーは死体になる
            let rest = self.energy[i];
            self.drop_carrion(self.xs[i], self.ys[i], rest);
            self.energy[i] = 0.0;
            self.die(i, death::INJURED);
        } else {
            self.energy[i] -= damage;
            self.ledger.lost_metabolism += damage as f64;
        }
    }

    fn hunt_odds(&self, i: usize, j: usize) -> HuntOdds {
        let p = &self.params;
        // 協力して狩る: 近くの仲間（同じ系統の肉食）が多いほど、獲物から見た自分が大きくなる
        let (rivals, allies) = self.carnivore_neighbors(i);
        let helpers = (allies.len() as f32).min(p.coop_max_allies);
        let effective = self.traits[i].size * (1.0 + p.coop_strength * helpers);
        let ratio = self.traits[j].size / effective;
        let by_size = 1.0 - ((ratio - p.hunt_easy_ratio) / (p.hunt_impossible_ratio - p.hunt_easy_ratio)).clamp(0.0, 1.0);
        // 大きさで不可能でも、仲間の数（反撃の強さ）は数えるので、ここでは打ち切らない
        let (x, y) = (self.xs[j], self.ys[j]);
        let prey_kind = self.kind[j];
        let r2 = p.guard_radius * p.guard_radius;
        let mut guards = 0u32;
        self.grid.for_each_within(x, y, p.guard_radius, |k, xk, yk, kind_k, _| {
            if k != j && kind_k == prey_kind && self.alive[k] {
                let dx = torus_delta(xk - x, self.width);
                let dy = torus_delta(yk - y, self.height);
                guards += (dx * dx + dy * dy < r2) as u32;
            }
        });
        // 肉食どうしの干渉（獲物の横取りや縄張り争い）: 近くに系統の違う肉食が多いほど成功しにくい
        // 逃げ: 獲物が前の tick に肉食から遠ざかっていた速さ（肉食から獲物への向きの成分）を、肉食の最高速度と比べる。
        // 気づいて逃げている獲物ほど仕留めにくい。止まっている獲物や、肉食に向かってくる獲物には効かない
        let (vx, vy) = self.prev_vel.get(j).copied().unwrap_or((0.0, 0.0));
        let dx = torus_delta(x - self.xs[i], self.width);
        let dy = torus_delta(y - self.ys[i], self.height);
        let d = (dx * dx + dy * dy).sqrt().max(1e-3);
        let away = ((vx * dx + vy * dy) / d).max(0.0);
        let escape = (away / self.traits[i].max_speed.max(1e-3)).min(1.0);
        let chance = p.hunt_success * by_size * (1.0 - p.escape_strength * escape)
            / (1.0 + p.guard_strength * guards as f32)
            / (1.0 + p.carn_interference * rivals as f32);
        HuntOdds { chance, escape, size_ratio: ratio, guards, allies: helpers as u32, rivals }
    }

    fn move_agent(&mut self, i: usize, speed: f32) {
        if speed == 0.0 {
            self.vel[i] = (0.0, 0.0);
            return;
        }
        self.life[i].distance += speed;
        let (x, y) = (self.xs[i], self.ys[i]);
        let nx = wrap(x + self.hx[i] * speed, self.width);
        let ny = wrap(y + self.hy[i] * speed, self.height);
        // 水には入れない。進む先が水なら、岸に沿って滑る（縦だけ・横だけの移動を試す）。
        // どちらもだめなら、その場で向きを反対にする
        let (nx, ny) = if !self.terrain.is_water(nx, ny) {
            (nx, ny)
        } else if !self.terrain.is_water(nx, y) {
            (nx, y)
        } else if !self.terrain.is_water(x, ny) {
            (x, ny)
        } else {
            self.hx[i] = -self.hx[i];
            self.hy[i] = -self.hy[i];
            (x, y)
        };
        self.vel[i] = (torus_delta(nx - x, self.width), torus_delta(ny - y, self.height));
        self.xs[i] = nx;
        self.ys[i] = ny;
    }

    /// 個体 i を死亡扱いにする。エネルギーは呼び出し側で 0 にしておくこと
    fn die(&mut self, i: usize, cause: u8) {
        // 同じ tick の中で、すでに死んだ個体がもう一度死ぬことがある（反撃で死んだ後の代謝など）。
        // 体を二重に死体にしないよう、二度目は何もしない
        if !self.alive[i] {
            return;
        }
        self.alive[i] = false;
        // 体は死体になる（食べられた場合は、狩った側が体ごと食べ、残りは try_hunt が死体にする）。お腹の子も体に含む
        if cause != death::EATEN {
            let body = self.body_energy(i);
            self.drop_carrion(self.xs[i], self.ys[i], body);
        }
        self.end_pregnancy(i);
        self.census.deaths[self.kind[i] as usize][cause as usize] += 1;
        self.step_deaths.push((self.id[i], cause));
        self.recent_deaths.push_back(DeathRecord { tick: self.tick, kind: self.kind[i], cause, hue: self.traits[i].hue });
        if self.growth[i] < 1.0 {
            self.census.juvenile_deaths[self.kind[i] as usize][cause as usize] += 1;
        }
        if self.kind[i] == CARNIVORE {
            self.census.carn_size_deaths[carn_size_bin(self.traits[i].adult_size)][cause as usize] += 1;
        }
        if self.kind[i] == HERBIVORE {
            let b = herb_size_bin(self.traits[i].adult_size);
            self.census.herb_size_deaths[b][cause as usize] += 1;
            if self.growth[i] < 1.0 {
                self.census.herb_size_juvenile_deaths[b][cause as usize] += 1;
            }
        }
        if self.id[i] == self.watch.id {
            self.watch.death = Some((cause, self.tick));
        }
    }

    /// エネルギー消費・加齢・死亡・繁殖
    fn live(&mut self, i: usize, speed: f32, wants_child: bool, mate: Mate) {
        self.tire(i, speed);
        let p = &self.params;
        let t = self.traits[i];
        let herb = self.kind[i] == HERBIVORE;
        let (metabolism, birth_cost) = if herb {
            (p.herb_metabolism, p.herb_birth_cost)
        } else {
            (p.carn_metabolism, p.carn_birth_cost)
        };

        // 代謝は大きさの 0.75 乗と視野と最高速度に、移動は大きさの約 0.7 乗と速度の 2 乗に比例する。
        // 持っている以上は失えない（エネルギーが負になって帳尻が狂うのを防ぐ）
        // 速く走れる体は、走っていなくても維持費がかかる（最高速度 × 大きさの 0.75 乗に比例）
        let upkeep = p.speed_upkeep * t.max_speed * t.size_pow075;
        // 体温を保つ費用: 好みの気温との差に比例し、大きいほど体積あたりは割安
        let temp_here = self.terrain.temperature_at(self.xs[i], self.ys[i]);
        let nest_here = self.nest[self.plants.cell_index(self.xs[i], self.ys[i])];
        let thermal = p.thermo_cost * (temp_here - t.temp_pref).abs() * t.thermal_scale * (1.0 - p.nest_insulation * nest_here);
        // 基本の代謝は動いている度合いに応じて変わる（止まっていれば rest_metabolism 倍。待ち伏せや休息で消費を抑えられる）
        let activity = (speed / t.max_speed).clamp(0.0, 1.0);
        let basal = metabolism * t.size_pow075 * (p.rest_metabolism + (1.0 - p.rest_metabolism) * activity);
        // 繁殖シグナルを出す費用
        let signal = p.signal_cost * self.signal[i] * t.size_pow075;
        // 目の維持費は見える面積（視野角 / 2 × 視野²）に比例。視野角 300°・視野 30px で vision_cost × 30 になるよう換算する
        // 目と脳の大きさも体に合わせて小さくなるので、維持費は大きさの 0.75 乗に比例する（大きさ 1 のときが基準）
        let eye = p.vision_cost * EYE_REF_RANGE * (0.5 * t.fov * t.vision * t.vision) / EYE_REF_AREA * t.size_pow075;
        // 脳の維持費は、使っている中間ニューロンの数に比例
        let brain = p.brain_cost * (t.brain.0 + t.brain.1) as f32 * t.size_pow075;
        // 記憶の維持費は記憶の長さに比例
        let memory = p.memory_cost * t.memory / 1000.0 * t.size_pow075;
        // 移動は大きさの move_size_power 乗（約 0.7 乗。体重あたりは大きいほど割安）と速度の 2 乗に比例する
        let movement = p.move_cost * t.size.max(0.01).powf(p.move_size_power) * speed * speed * (1.0 + self.gestation_load(i));
        let parts = [basal, upkeep, thermal, signal, eye, brain, memory, movement];
        let cost = parts.iter().sum::<f32>().min(self.energy[i]);
        // 内訳は、持っている分で頭打ちになったときは同じ割合で縮めて数える
        let scale = cost / parts.iter().sum::<f32>().max(1e-12);
        for (sum, part) in self.census.costs[self.kind[i] as usize].iter_mut().zip(parts) {
            *sum += (part * scale) as f64;
        }
        self.energy[i] -= cost;
        self.ledger.lost_metabolism += cost as f64;
        self.age[i] += 1;
        self.cooldown[i] = self.cooldown[i].saturating_sub(1);
        self.rest[i] = self.rest[i].saturating_sub(1);
        self.attacked[i] = self.attacked[i].saturating_sub(1);
        // 覚えている巣が崩れて消えていたら忘れる
        if self.home[i] != NO_HOME && self.nest[self.home[i] as usize] <= 0.0 {
            self.home[i] = NO_HOME;
        }
        // 成熟までの時間は大人の大きさの 0.25 乗に比例（小さい体ほど早く大人になる）
        let maturity = if herb { p.herb_maturity } else { p.carn_maturity } * t.adult_pow025;
        self.grow(i, maturity);
        if self.id[i] == self.watch.id {
            self.watch.speed = speed;
            self.watch.cost = cost;
            let e = self.energy[i];
            let hist = &mut self.watch.energy_history;
            if hist.len() >= ENERGY_HISTORY {
                hist.remove(0);
            }
            hist.push(e);
        }

        if self.energy[i] <= 0.0 {
            self.energy[i] = 0.0;
            self.die(i, death::STARVED);
            return;
        }
        if self.age[i] as f32 > self.lifespan(i) {
            // 残っていたエネルギーは死体になる
            let rest = self.energy[i];
            self.drop_carrion(self.xs[i], self.ys[i], rest);
            self.energy[i] = 0.0;
            self.die(i, death::OLD_AGE);
            return;
        }
        // 妊娠期間が終わったら産む
        if self.gest_due[i] != 0 && self.tick >= self.gest_due[i] {
            self.give_birth(i);
        }
        // 成熟するまで繁殖できない。繁殖したあとしばらくも繁殖できない
        if self.can_breed(i) {
            let k = if !wants_child { 0 } else if matches!(mate, Mate::Missing) { 1 } else { 2 };
            self.census.ready[self.kind[i] as usize][k] += 1;
        }
        if wants_child && self.can_breed(i) {
            // 交配を試みたら（成功しても失敗しても）、しばらく次の繁殖はできない
            if !matches!(mate, Mate::Missing) {
                let rest = if herb { self.params.herb_repro_rest } else { self.params.carn_repro_rest };
                self.rest[i] = rest as u16;
            }
            match mate {
                Mate::Partner(j) => {
                    self.matings += 1;
                    self.life[i].matings += 1;
                    if self.rng.f32() < self.mating_chance(i, j) {
                        self.reproduce(i, birth_cost * t.adult_size, Some(j));
                    } else {
                        self.life[i].failed += 1;
                        // 相性が悪く失敗した。繁殖のコストは払う（払わないと、相手を選ぶ利益が生まれない）
                        self.failed_matings += 1;
                        let cost = (birth_cost * t.adult_size).min(self.energy[i]);
                        self.energy[i] -= cost;
                        self.ledger.lost_birth += cost as f64;
                    }
                }
                Mate::Asexual => {
                    self.life[i].matings += 1;
                    self.reproduce(i, birth_cost * t.adult_size, None);
                }
                Mate::Missing => {} // 相手がいなければ繁殖できない
            }
        }
    }

    /// i と j が繁殖に成功する確率（遺伝的に遠いほど下がる）
    pub fn mating_chance(&self, i: usize, j: usize) -> f32 {
        let p = &self.params;
        let d = genome::genetic_distance(
            self.genome(i),
            self.genome(j),
            p.mutation_trait_sigma,
            p.mutation_weight_sigma,
            p.mutation_temp_sigma,
        );
        genome::compatibility(d, p.compat_full_distance, p.compat_zero_distance)
    }

    /// 成長させる。maturity は成長度 0 から 1 になるまでの tick 数
    fn grow(&mut self, i: usize, maturity: f32) {
        if self.growth[i] >= 1.0 {
            return;
        }
        // 育った分の体のエネルギーを蓄えから払う。払えなければこの tick は育たない
        let step = (1.0 / maturity).min(1.0 - self.growth[i]);
        let cost = self.params.body_energy * self.traits[i].adult_size * step;
        if self.energy[i] < cost {
            return;
        }
        self.energy[i] -= cost;
        self.growth[i] += step;
        if self.growth[i] > 1.0 - 1e-6 {
            self.growth[i] = 1.0;
        }
        // いまの大きさから決まる値（0.75 乗など）の計算は重いので、8 tick ごとと成熟したときだけ行う
        if self.age[i] % 8 == 0 || self.growth[i] >= 1.0 {
            let g = self.growth[i];
            self.traits[i].grow_to(g, self.params.max_turn);
        }
    }

    /// 繁殖コストを引いた残りのうち、子への投資の割合を子に渡す。一度に産む数（遺伝子）だけ産み、渡すエネルギーは等分する。
    /// エネルギーを払うのは繁殖を決めた側（i）だけで、相手（mate）は遺伝子を渡すだけ。
    /// 次に繁殖できるまでの休みは、子に渡したエネルギーの合計に比例する（妊娠と子育てにあたる）
    fn reproduce(&mut self, i: usize, birth_cost: f32, mate: Option<usize>) {
        let before = self.energy[i];
        let available = before - birth_cost;
        let given = available * self.traits[i].invest;
        let parent = available - given;
        self.energy[i] = parent;
        self.ledger.lost_birth += (before - parent - given) as f64;
        // 一度に産む数: 整数部分は必ず、端数は確率で 1 体増やす
        let mean = self.traits[i].litter;
        let litter = mean.floor() as u32 + (self.rng.f32() < mean.fract()) as u32;
        // 休み = 基本の休み × 渡した量 ÷ (繁殖に必要な量の半分)。0.1〜3 倍に収める。妊娠期間も同じ比で伸び縮みする
        let herb = self.kind[i] == HERBIVORE;
        let base_rest = if herb { self.params.herb_repro_rest } else { self.params.carn_repro_rest };
        let ratio = (given / (0.5 * self.repro_threshold(i))).clamp(0.1, 3.0);
        self.rest[i] = (base_rest * ratio) as u16;
        let gestation = if herb { self.params.herb_gestation } else { self.params.carn_gestation } * self.traits[i].adult_pow025 * ratio;

        // 身ごもる。相手の遺伝子は産むときまで写しておく（相手が死んでも子は生まれる）
        self.gest_energy[i] = given;
        self.gest_litter[i] = litter.min(u8::MAX as u32) as u8;
        self.gest_mate[i] = mate.map(|j| self.id[j]).unwrap_or(0);
        self.gest_due[i] = self.tick + (gestation.max(0.0) as u64).max(1);
        if let Some(j) = mate {
            let slot = match self.free_slots.pop() {
                Some(s) => s,
                None => {
                    self.mate_genomes.resize(self.mate_genomes.len() + GENOME_LEN, 0.0);
                    (self.mate_genomes.len() / GENOME_LEN - 1) as u32
                }
            };
            let at = slot as usize * GENOME_LEN;
            self.mate_genomes[at..at + GENOME_LEN].copy_from_slice(&self.genomes[j * GENOME_LEN..(j + 1) * GENOME_LEN]);
            self.gest_slot[i] = slot;
        }
        if gestation < 1.0 {
            self.give_birth(i);
        }
    }

    /// 妊娠を終える（産んだか、親が死んだ）。お腹の子のエネルギーは呼ぶ側で扱う
    fn end_pregnancy(&mut self, i: usize) {
        if self.gest_slot[i] != NO_SLOT {
            self.free_slots.push(self.gest_slot[i]);
            self.gest_slot[i] = NO_SLOT;
        }
        self.gest_due[i] = 0;
        self.gest_energy[i] = 0.0;
        self.gest_litter[i] = 0;
        self.gest_mate[i] = 0;
    }

    /// お腹の子を産む
    fn give_birth(&mut self, i: usize) {
        let given = self.gest_energy[i];
        let litter = self.gest_litter[i].max(1) as u32;
        let mate_id = self.gest_mate[i];
        let slot = self.gest_slot[i];
        self.children[i] += litter;

        let m = Mutation {
            rate: self.params.mutation_rate,
            trait_rate: self.params.mutation_trait_rate,
            trait_sigma: self.params.mutation_trait_sigma,
            weight_sigma: self.params.mutation_weight_sigma,
            hue_sigma: self.params.mutation_hue_sigma,
            temp_sigma: self.params.mutation_temp_sigma,
            neuron_flip: self.params.mutation_neuron,
            size_sigma: if self.kind[i] == CARNIVORE { self.params.mutation_carn_size_sigma } else { self.params.mutation_trait_sigma },
        };
        let spread = self.params.birth_spread;
        // 産んだ場所を、子が育つまで覚えておく
        let maturity = if self.kind[i] == HERBIVORE { self.params.herb_maturity } else { self.params.carn_maturity };
        self.den[i] = (self.xs[i], self.ys[i]);
        self.den_until[i] = self.tick + (maturity * self.traits[i].adult_pow025) as u64;
        // きょうだいは、それぞれ別に交叉と変異をする（遺伝子が少しずつ違う）
        for _ in 0..litter {
            let genome_at = self.births_genomes.len();
            let mine = &self.genomes[i * GENOME_LEN..(i + 1) * GENOME_LEN];
            if slot != NO_SLOT {
                let at = slot as usize * GENOME_LEN;
                let theirs = &self.mate_genomes[at..at + GENOME_LEN];
                genome::crossover(mine, theirs, &mut self.rng, &mut self.births_genomes);
            } else {
                self.births_genomes.extend_from_slice(mine);
            }
            genome::mutate(&mut self.births_genomes[genome_at..], &m, &mut self.rng);
            let x = self.xs[i] + self.rng.range(-spread, spread);
            let y = self.ys[i] + self.rng.range(-spread, spread);
            self.census.births[self.kind[i] as usize] += 1;
            if self.kind[i] == HERBIVORE {
                self.census.herb_size_births[herb_size_bin(self.traits[i].adult_size)] += 1;
            } else {
                self.census.carn_size_births[carn_size_bin(self.traits[i].adult_size)] += 1;
            }
            self.births.push(Birth {
                parents: [self.id[i], mate_id],
                x,
                y,
                parent_x: self.xs[i],
                parent_y: self.ys[i],
                kind: self.kind[i],
                energy: given / litter as f32,
                generation: self.generation[i] + 1,
                genome_at,
                home: self.home[i],
            });
        }
        // お腹の子のエネルギーは births に移った
        self.end_pregnancy(i);
    }

    fn apply_births(&mut self) {
        let births = std::mem::take(&mut self.births);
        let genomes = std::mem::take(&mut self.births_genomes);
        for b in &births {
            let g = &genomes[b.genome_at..b.genome_at + GENOME_LEN];
            let traits = genome::traits(g, self.params.max_turn);
            // 生まれたときの成長度は、受け取ったエネルギーが大人の繁殖に必要な量の何割か。
            // 子への投資が多いほど大きく生まれる
            let adult_need = self.base_repro_threshold(b.kind) * traits.adult_size;
            let mut growth = (b.energy / adult_need).clamp(self.params.min_birth_growth, 1.0);
            // 受け取ったエネルギーで体を作り、残りが蓄えになる。体に使えるのは受け取った量の 8 割まで（足りなければ小さく生まれる）
            let body_full = self.params.body_energy * traits.adult_size;
            if body_full * growth > 0.8 * b.energy {
                growth = (0.8 * b.energy / body_full.max(1e-6)).max(0.02);
            }
            let body = body_full * growth;
            let reserve = b.energy - body;
            let (mut x, mut y) = (wrap(b.x, self.width), wrap(b.y, self.height));
            if self.terrain.is_water(x, y) {
                (x, y) = (b.parent_x, b.parent_y);
            }
            self.census.birth_growth[b.kind as usize] += growth as f64;
            self.spawn_grown(x, y, b.kind, reserve, b.generation, g, traits, growth);
            *self.parents.last_mut().unwrap() = b.parents;
            *self.home.last_mut().unwrap() = b.home;
        }
        self.births = births;
        self.births.clear();
        self.births_genomes = genomes;
        self.births_genomes.clear();
    }

    /// 死んだ個体を取り除く。順番は保つ（決定論のため）
    fn remove_dead(&mut self) {
        // 後ろから見ていき、死んだ個体の場所に末尾の個体を移す（swap_remove）。
        // 順番を保って詰めると、遺伝子（1 体 GENOME_LEN 個）を毎 tick ほぼ全員分コピーすることになり重い。
        // この方法でも同じ入力なら同じ結果になる（決定論は保たれる）
        for i in (0..self.count()).rev() {
            if self.alive[i] {
                continue;
            }
            self.id.swap_remove(i);
            self.xs.swap_remove(i);
            self.ys.swap_remove(i);
            self.hx.swap_remove(i);
            self.hy.swap_remove(i);
            self.energy.swap_remove(i);
            self.age.swap_remove(i);
            self.kind.swap_remove(i);
            self.state.swap_remove(i);
            self.generation.swap_remove(i);
            self.children.swap_remove(i);
            self.traits.swap_remove(i);
            self.growth.swap_remove(i);
            self.cooldown.swap_remove(i);
            self.rest.swap_remove(i);
            self.attacked.swap_remove(i);
            self.alone.swap_remove(i);
            self.memory.swap_remove(i);
            self.home.swap_remove(i);
            self.vel.swap_remove(i);
            self.stamina.swap_remove(i);
            self.den.swap_remove(i);
            self.den_until.swap_remove(i);
            self.gest_due.swap_remove(i);
            self.gest_energy.swap_remove(i);
            self.gest_litter.swap_remove(i);
            self.gest_mate.swap_remove(i);
            self.gest_slot.swap_remove(i);
            self.vigilance.swap_remove(i);
            self.natal.swap_remove(i);
            self.cover.swap_remove(i);
            self.signal.swap_remove(i);
            self.parents.swap_remove(i);
            self.life.swap_remove(i);
            self.alive.swap_remove(i);
            let last = self.genomes.len() - GENOME_LEN;
            if i * GENOME_LEN != last {
                self.genomes.copy_within(last.., i * GENOME_LEN);
            }
            self.genomes.truncate(last);
        }
    }

    pub fn fill_render(&mut self) {
        self.render.resize(self.count() * RENDER_STRIDE, 0.0);
        for i in 0..self.count() {
            let o = i * RENDER_STRIDE;
            self.render[o] = self.xs[i];
            self.render[o + 1] = self.ys[i];
            self.render[o + 2] = self.kind[i] as f32;
            self.render[o + 3] = self.traits[i].size;
            self.render[o + 4] = self.traits[i].hue;
            // 向き（ラジアン）。描画で回すのは肉食（三角形）だけなので、計算も肉食だけ
            self.render[o + 5] = if self.kind[i] == CARNIVORE { self.hy[i].atan2(self.hx[i]) } else { 0.0 };
        }
    }

    /// 種類ごとの繁殖シグナルの平均（いなければ 0）
    pub fn mean_signal(&self, kind: u8) -> f64 {
        let (mut sum, mut n) = (0.0f64, 0u32);
        for i in 0..self.count() {
            if self.kind[i] == kind {
                sum += self.signal[i] as f64;
                n += 1;
            }
        }
        if n == 0 { 0.0 } else { sum / n as f64 }
    }

    /// 種類ごとの量（いまの体の大きさの合計）
    pub fn biomass(&self, kind: u8) -> f64 {
        (0..self.count()).filter(|&i| self.kind[i] == kind).map(|i| self.traits[i].size as f64).sum()
    }

    /// 系統（種類 kind のうち、標識色が hue_from から幅 hue_width の範囲にある個体）の統計。
    /// 色相は 1 周でつながるので、範囲は 1 をまたいでもよい。幅 1 以上なら種類全体。並びは LINEAGE_STATS のとおり
    pub fn lineage_stats(&self, kind: u8, hue_from: f32, hue_width: f32) -> [f64; LINEAGE_STATS] {
        let mut out = [0.0f64; LINEAGE_STATS];
        let all = hue_width >= 1.0;
        let (tw, th) = (std::f64::consts::TAU / self.width as f64, std::f64::consts::TAU / self.height as f64);
        // 位置の平均はトーラスなので、座標を角度にして cos と sin の平均から求める
        let (mut cx, mut sx, mut cy, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        // 直近の死因の内訳（死んだ個体は数に入らないので、個体がいなくても数える）
        for d in &self.recent_deaths {
            if d.kind == kind && (all || (d.hue - hue_from).rem_euclid(1.0) < hue_width) && (1..=7).contains(&d.cause) {
                out[17 + d.cause as usize] += 1.0;
            }
        }
        let members: Vec<usize> = (0..self.count())
            .filter(|&i| self.kind[i] == kind && (all || (self.traits[i].hue - hue_from).rem_euclid(1.0) < hue_width))
            .collect();
        let n = members.len() as f64;
        out[0] = n;
        if members.is_empty() {
            return out;
        }
        for &i in &members {
            let t = &self.traits[i];
            out[1] += (self.growth[i] < 1.0) as u8 as f64;
            out[2] += t.adult_size as f64;
            out[3] += t.max_speed as f64;
            out[4] += t.vision as f64;
            out[5] += t.fov.to_degrees() as f64;
            out[6] += t.temp_pref as f64;
            out[7] += t.invest as f64;
            out[8] += self.signal[i] as f64;
            out[9] += self.generation[i] as f64;
            out[13] += (self.energy[i] / self.repro_threshold(i)) as f64;
            // 寒い半分（緯度が負の側。上下の端に近い）
            out[14] += ((std::f32::consts::TAU * self.ys[i] / self.height).cos() > 0.0) as u8 as f64;
            out[15] += t.brain.0 as f64;
            out[16] += t.brain.1 as f64;
            out[17] += t.litter as f64; // 平均（端数込み）
            let (ax, ay) = (self.xs[i] as f64 * tw, self.ys[i] as f64 * th);
            cx += ax.cos();
            sx += ax.sin();
            cy += ay.cos();
            sy += ay.sin();
        }
        for k in [1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 14, 15, 16, 17] {
            out[k] /= n;
        }
        let mx = sx.atan2(cx).rem_euclid(std::f64::consts::TAU) / tw;
        let my = sy.atan2(cy).rem_euclid(std::f64::consts::TAU) / th;
        out[10] = mx;
        out[11] = my;
        // 広がり: 重心からの距離の平均
        out[12] = members
            .iter()
            .map(|&i| {
                let dx = torus_delta(self.xs[i] - mx as f32, self.width) as f64;
                let dy = torus_delta(self.ys[i] - my as f32, self.height) as f64;
                (dx * dx + dy * dy).sqrt()
            })
            .sum::<f64>()
            / n;
        out
    }

    /// 系統（lineage_stats と同じ範囲）の中からランダムに 1 体の id を返す。いなければ 0。
    /// 観察のための操作なので、シミュレーションの乱数は使わない（使うと歴史が変わってしまう）。nonce は呼ぶたびに変える値
    pub fn random_in_lineage(&self, kind: u8, hue_from: f32, hue_width: f32, nonce: u64) -> u32 {
        let all = hue_width >= 1.0;
        let members: Vec<usize> = (0..self.count())
            .filter(|&i| self.kind[i] == kind && (all || (self.traits[i].hue - hue_from).rem_euclid(1.0) < hue_width))
            .collect();
        if members.is_empty() {
            return 0;
        }
        let mut r = Rng::new(nonce ^ self.tick.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        self.id[members[(r.f32() * members.len() as f32) as usize % members.len()]]
    }

    /// 種類 kind の大人の大きさの分布。大きさの範囲（genome::SIZE_RANGE）を、対数で等間隔に SIZE_HIST_BINS 段階に分けて数える
    pub fn adult_size_histogram(&self, kind: u8) -> [u32; SIZE_HIST_BINS] {
        let mut out = [0u32; SIZE_HIST_BINS];
        for i in 0..self.count() {
            if self.alive[i] && self.kind[i] == kind {
                out[size_hist_bin(self.traits[i].adult_size)] += 1;
            }
        }
        out
    }

    /// 種類ごとの形質の平均。[個体数, 大きさ, 最高速度, 視野, 子への投資, 世代]
    pub fn trait_means(&self, kind: u8) -> [f64; 6] {
        let mut sum = [0.0f64; 6];
        for i in 0..self.count() {
            if self.kind[i] != kind {
                continue;
            }
            let t = &self.traits[i];
            sum[0] += 1.0;
            sum[1] += t.adult_size as f64;
            sum[2] += t.max_speed as f64;
            sum[3] += t.vision as f64;
            sum[4] += t.invest as f64;
            sum[5] += self.generation[i] as f64;
        }
        let n = sum[0];
        if n > 0.0 {
            for v in &mut sum[1..] {
                *v /= n;
            }
        }
        sum
    }

    /// 群れの度合い。[集まり具合（Clark-Evans 比）, 向きのそろい具合, 同じ系統どうしのそろい具合,
    /// 違う系統どうしのそろい具合, 近くの仲間のうち同じ系統の割合]
    ///
    /// - 集まり具合: 最寄りの仲間までの平均距離 ÷ 同じ密度でランダムに散らばった場合の期待値（0.5 / √密度）。
    ///   1 ならランダム、1 未満なら集まっている、1 より大きければ互いに避けている
    /// - 向きのそろい具合: 自分と、半径 HERD_RADIUS 内の仲間の進行方向の cos の平均。0 ならばらばら、1 なら全員同じ向き
    ///
    /// 集まり具合だけだと「草の多い場所に別々に集まっている」のと区別できないので、向きも見る。
    /// 同じ系統（標識色の差が hue_similar 以下）どうしでだけ向きがそろっていれば、系統の群れと言える。
    /// 計算を軽くするため、HERD_SAMPLE_STEP 体に 1 体だけ調べる。
    /// グリッドを今の位置で作り直す（step の最初にも作り直すので、シミュレーションには影響しない）
    pub fn herd_stats(&mut self, kind: u8) -> [f64; 5] {
        const HERD_RADIUS: f32 = 32.0;
        const HERD_SAMPLE_STEP: usize = 4;
        /// 最寄りの仲間を探す範囲。これより遠ければこの距離として数える（疎な種類ではやや過小になる）
        const NN_LIMIT: f32 = 128.0;

        self.grid.build(&self.xs, &self.ys, &self.kind, &self.cover);
        let n_kind = self.count_kind(kind);
        if n_kind < 2 {
            return [f64::NAN; 5];
        }
        let (w, h) = (self.width, self.height);
        let (mut nn_sum, mut nn_n) = (0.0f64, 0usize);
        let (mut align_sum, mut align_n) = (0.0f64, 0usize);
        // [同じ系統, 違う系統] それぞれの向きの cos の合計と数
        let (mut lin_sum, mut lin_n) = ([0.0f64; 2], [0usize; 2]);
        let hue_similar = self.params.hue_similar;
        for i in (0..self.count()).step_by(HERD_SAMPLE_STEP) {
            if self.kind[i] != kind {
                continue;
            }
            let (x, y) = (self.xs[i], self.ys[i]);
            let (hx, hy) = (self.hx[i], self.hy[i]);
            let my_hue = self.traits[i].hue;
            let mut best = NN_LIMIT * NN_LIMIT;
            self.grid.for_each_within(x, y, NN_LIMIT, |j, xj, yj, kind_j, _| {
                if j == i || kind_j != kind {
                    return;
                }
                let dx = torus_delta(xj - x, w);
                let dy = torus_delta(yj - y, h);
                let d2 = dx * dx + dy * dy;
                best = best.min(d2);
                if d2 < HERD_RADIUS * HERD_RADIUS {
                    let c = (hx * self.hx[j] + hy * self.hy[j]) as f64;
                    align_sum += c;
                    align_n += 1;
                    let other = (genome::hue_distance(my_hue, self.traits[j].hue) > hue_similar) as usize;
                    lin_sum[other] += c;
                    lin_n[other] += 1;
                }
            });
            nn_sum += (best as f64).sqrt();
            nn_n += 1;
        }
        // 個体は陸にしかいないので、密度は陸の面積で割る
        let density = n_kind as f64 / self.terrain.land_area() as f64;
        let expected = 0.5 / density.sqrt();
        let ratio = if nn_n > 0 { nn_sum / nn_n as f64 / expected } else { f64::NAN };
        let mean = |s: f64, n: usize| if n > 0 { s / n as f64 } else { f64::NAN };
        let same_share = mean(lin_n[0] as f64, align_n);
        [ratio, mean(align_sum, align_n), mean(lin_sum[0], lin_n[0]), mean(lin_sum[1], lin_n[1]), same_share]
    }
}

/// 行きたい方向 (fwd, right)（自分基準）へ、最大旋回角までだけ向きを変えた後の向き（自分基準の単位ベクトル）。
/// turn_cos, turn_sin は最大旋回角の cos と sin
fn steer(fwd: f32, right: f32, turn_cos: f32, turn_sin: f32) -> (f32, f32) {
    let len = (fwd * fwd + right * right).sqrt();
    if len < 1e-6 {
        return (1.0, 0.0); // 行きたい方向がなければ、まっすぐ
    }
    let (f, r) = (fwd / len, right / len);
    if f >= turn_cos {
        (f, r) // 最大旋回角より内側なので、そのまま向ける
    } else if r >= 0.0 {
        (turn_cos, turn_sin) // 右へ最大まで
    } else {
        (turn_cos, -turn_sin) // 左へ最大まで
    }
}

/// 獲物の大きさの好み（0〜1）。大きさの比 ratio が好みの範囲 [min, max] の中なら 1。
/// 外れるほど、外れ方（比の対数）に応じて exp(−(外れ方 ÷ softness)²) で下がる。肉食が獲物に気づく距離に掛ける
fn prey_preference(ratio: f32, min: f32, max: f32, softness: f32) -> f32 {
    let off = if ratio < min {
        (min / ratio.max(1e-6)).ln()
    } else if ratio > max {
        (ratio / max).ln()
    } else {
        return 1.0;
    };
    let t = off / softness.max(1e-6);
    (-t * t).exp()
}

/// 座標を [0, size) に折り返す（トーラス世界）
fn wrap(v: f32, size: f32) -> f32 {
    if v < 0.0 {
        v + size
    } else if v >= size {
        v - size
    } else {
        v
    }
}

/// トーラス上での最短の差分
pub fn torus_delta(d: f32, size: f32) -> f32 {
    if d > size * 0.5 {
        d - size
    } else if d < -size * 0.5 {
        d + size
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_params() -> Params {
        let mut p = Params::default();
        // テストは地形の影響を受けないよう、平らな世界にする（地形自体は terrain のテストで確かめる）
        p.water_fraction = 0.0;
        p.fertility_min = 1.0;
        p.world_width = 640.0;
        p.world_height = 480.0;
        p.herb_initial = 600.0;
        p.carn_initial = 40.0;
        // 繁殖のテストは交配した tick に産むとして書いてある（妊娠は専用のテストで確かめる）
        p.herb_gestation = 0.0;
        p.carn_gestation = 0.0;
        p
    }

    #[test]
    fn 初期個体を組に分けると_組ごとに産む数と投資が決まり_東西の帯に並ぶ() {
        let mut p = small_params();
        p.founder_groups = 4.0;
        let w = World::new(1, p.clone());
        let band = p.world_width / 4.0;
        for kind in [HERBIVORE, CARNIVORE] {
            let mut values = [None; 4];
            for i in (0..w.count()).filter(|&i| w.kind[i] == kind) {
                let k = (w.xs[i] / band) as usize;
                let t = &w.traits[i];
                assert!((p.founder_litter_min..=p.founder_litter_max).contains(&t.litter));
                assert!((p.founder_invest_min..=p.founder_invest_max).contains(&t.invest));
                let v = *values[k].get_or_insert((t.litter, t.invest));
                assert_eq!(v, (t.litter, t.invest), "同じ帯（組）の個体は同じ値");
            }
            let distinct: Vec<_> = values.iter().flatten().collect();
            assert!(distinct.windows(2).any(|v| v[0] != v[1]), "組ごとに値が違う");
        }
        // 分けなければ、全員が既定の値
        let w = World::new(1, small_params());
        assert!((0..w.count()).all(|i| w.traits[i].litter == 1.0 && w.traits[i].invest == 0.5));
    }

    #[test]
    fn 同じシードなら同じ歴史になる() {
        let mut a = World::new(7, small_params());
        let mut b = World::new(7, small_params());
        for _ in 0..300 {
            a.step();
            b.step();
        }
        assert_eq!(a.xs, b.xs);
        assert_eq!(a.energy, b.energy);
        assert_eq!(a.genomes, b.genomes);
    }

    #[test]
    fn エネルギーの帳尻が合う() {
        // 巣の材料にした植物も帳簿に入ることを確かめるため、巣を作る初期配線を入れる
        let mut p = small_params();
        p.nest_wiring = 2.0;
        let mut w = World::new(3, p);
        for _ in 0..2000 {
            w.step();
        }
        let expected = w.ledger.expected_total();
        let actual = w.total_energy();
        let rel = (expected - actual).abs() / expected.abs().max(1.0);
        assert!(
            rel < 1e-4,
            "帳簿上 {expected:.3} / 実際 {actual:.3}（相対誤差 {rel:e}）\n{:?}",
            w.ledger
        );
    }

    #[test]
    fn 座標は世界の中に収まる() {
        let mut w = World::new(9, small_params());
        for _ in 0..500 {
            w.step();
        }
        assert!(w.xs.iter().all(|&x| (0.0..640.0).contains(&x)));
        assert!(w.ys.iter().all(|&y| (0.0..480.0).contains(&y)));
    }

    #[test]
    fn 日射がなければ植物は増えない() {
        let mut p = small_params();
        p.sunlight = 0.0;
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        let mut w = World::new(1, p);
        let before = w.plants.total();
        for _ in 0..100 {
            w.step();
        }
        assert_eq!(w.plants.total(), before);
    }

    #[test]
    fn 死んだ個体のエネルギーは残らない() {
        let mut w = World::new(5, small_params());
        for _ in 0..500 {
            w.step();
            assert!(w.energy.iter().all(|&e| e > 0.0));
        }
    }

    #[test]
    fn 遺伝子と形質は個体と一緒に詰め直される() {
        let mut w = World::new(4, small_params());
        for _ in 0..400 {
            w.step();
            assert_eq!(w.genomes.len(), w.count() * GENOME_LEN);
            for i in (0..w.count()).step_by(37) {
                let t = genome::traits(w.genome(i), w.params.max_turn);
                assert_eq!(t.adult_size, w.traits[i].adult_size, "個体 {i} の遺伝子と形質がずれている");
                assert_eq!(t.hue, w.traits[i].hue);
            }
        }
    }

    #[test]
    fn 草むらにいる相手は見つけにくい() {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        let mut w = World::new(1, p);
        let g = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(130.0, 100.0, HERBIVORE, 5.0, 0, &g);
        face(&mut w, 0, 1.0, 0.0);
        w.params.cover_strength = 0.6;

        w.cover = vec![0.0, 0.0];
        w.build_grid();
        assert!(w.sense(0).prey.is_some(), "草がなければ 30px 先の獲物が見える");
        // 草が満杯なら視野 40 × (1 - 0.6) = 16px までしか見えない
        w.cover = vec![0.0, 1.0];
        w.build_grid();
        assert!(w.sense(0).prey.is_none(), "草むらの獲物は見えない");
    }

    #[test]
    fn idは重複せず死亡と詰め直しの後も個体についていく() {
        let mut w = World::new(11, small_params());
        let target = *w.id.last().unwrap();
        w.select(target);
        let mut last_age = *w.age.last().unwrap();
        for _ in 0..200 {
            w.step();
            let mut ids = w.id.clone();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), w.count(), "id が重複している");
            match w.index_of(target) {
                Some(i) => {
                    assert_eq!(w.age[i], last_age + 1, "別の個体を指している");
                    last_age = w.age[i];
                }
                None => {
                    assert!(w.watch.death.is_some(), "死因が記録されずに消えた");
                    break;
                }
            }
        }
    }

    /// 草食 0 と相手 1 を置き、0 を身ごもらせた世界（妊娠期間 100 tick）
    fn pregnant_world() -> World {
        let mut w = empty_world();
        w.params.herb_gestation = 100.0;
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        w.spawn(103.0, 100.0, HERBIVORE, 15.0, 0, &g);
        w.ledger.initial = w.total_energy();
        w.reproduce(0, 1.0, Some(1));
        w
    }

    #[test]
    fn 身ごもると重くなり_期間が過ぎると相手が死んでいても産む() {
        let mut w = pregnant_world();
        let (mother, father) = (w.id[0], w.id[1]);
        assert_eq!(w.count(), 2, "すぐには産まない");
        assert!(w.gest_due[0] > w.tick && w.gest_energy[0] > 0.0);
        // 渡した量 ÷ 大人の体のエネルギー × 0.1 だけ重い。最高速度は 1 / (1 + 重さ) 倍
        let load = w.params.gestation_load * w.gest_energy[0] / (w.params.body_energy * w.traits[0].adult_size);
        assert!((w.gestation_load(0) - load).abs() < 1e-6 && load > 0.05, "重さ {load}");
        assert!(w.brain_inputs()[0][genome::IN_GESTATION] > 0.0);
        assert!(!w.can_breed(0), "妊娠中は繁殖できない");
        // 相手が死んでも、写しておいた遺伝子で子は生まれる
        w.ledger.lost_death += w.energy[1] as f64; // 蓄えは消えたことにする（帳簿の上で）
        w.energy[1] = 0.0;
        w.die(1, death::OLD_AGE);
        let due = w.gest_due[0];
        while w.tick < due + 1 {
            w.step();
        }
        let m = w.index_of(mother).unwrap();
        assert!(w.count() >= 2, "産んだ");
        let child = (0..w.count()).find(|&k| w.parents[k][0] == mother).expect("子がいる");
        assert_eq!(w.parents[child], [mother, father]);
        assert_eq!((w.gest_due[m], w.gest_energy[m], w.gestation_load(m)), (0, 0.0, 0.0), "産んだら軽くなる");
        let gap = (w.total_energy() - w.ledger.expected_total()).abs();
        assert!(gap < 1e-3, "帳簿が合う（差 {gap}）");
        // 相手の遺伝子の置き場所は空く
        assert_eq!(w.free_slots.len(), 1);
    }

    #[test]
    fn 妊娠中に死ぬとお腹の子も死体になる() {
        let mut w = pregnant_world();
        let carried = w.gest_energy[0];
        let before = w.carrion.iter().sum::<f32>();
        w.ledger.lost_death += w.energy[0] as f64;
        w.energy[0] = 0.0;
        w.die(0, death::FOUGHT);
        let dropped = w.carrion.iter().sum::<f32>() - before;
        assert!(dropped >= carried + w.params.body_energy - 1e-4, "体とお腹の子が死体になる: {dropped}");
        w.step();
        assert_eq!(w.count(), 1, "子は生まれない");
        let gap = (w.total_energy() - w.ledger.expected_total()).abs();
        assert!(gap < 1e-3, "帳簿が合う（差 {gap}）");
    }

    #[test]
    fn 相手がいなければ繁殖しない() {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        p.sunlight = 0.0;
        let mut w = World::new(1, p);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        // 1 体だけ、繁殖に十分なエネルギーを持たせる
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        for _ in 0..50 {
            w.step();
        }
        assert_eq!(w.count(), 1, "相手がいないのに繁殖した");

        // 近くに相手がいれば繁殖する
        w.spawn(w.xs[0] + 3.0, w.ys[0], HERBIVORE, 5.0, 0, &g);
        w.energy[0] = 15.0;
        w.step();
        assert!(w.count() > 2, "相手がいるのに繁殖しなかった");
    }

    /// 個体 i を (dx, dy) の向きにする
    fn face(w: &mut World, i: usize, dx: f32, dy: f32) {
        let len = (dx * dx + dy * dy).sqrt();
        w.hx[i] = dx / len;
        w.hy[i] = dy / len;
    }

    fn empty_world() -> World {
        let mut p = small_params();
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        p.sunlight = 0.0;
        World::new(1, p)
    }

    #[test]
    fn 大きい獲物ほど狩りに失敗しやすい() {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        for (k, size) in [0.5, 1.0, 1.4, 2.0].iter().enumerate() {
            let g = genome::initial(true, *size, 1.0, 30.0, 0.0);
            // 獲物同士が守り合わないよう、離して置く
            w.spawn(300.0 + 80.0 * k as f32, 300.0, HERBIVORE, 5.0, 0, &g);
        }
        w.build_grid();
        let chances: Vec<f32> = (1..=4).map(|j| w.hunt_chance(0, j)).collect();
        assert_eq!(chances[0], w.params.hunt_success, "十分小さい獲物は最大の成功率");
        assert!(chances[1] < chances[0] && chances[2] < chances[1], "大きいほど下がる: {chances:?}");
        assert_eq!(chances[3], 0.0, "大きすぎる獲物は狩れない");
    }

    /// 肉食 1 体（番号 0）と、遠くに獲物 1 体（番号 1、大きさ prey_size）を置いた世界
    fn hunting_world(prey_size: f32) -> World {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.5);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        let herb = genome::initial(true, prey_size, 1.0, 30.0, 0.0);
        w.spawn(104.0, 100.0, HERBIVORE, 5.0, 0, &herb);
        w
    }

    #[test]
    fn 仲間がいれば1体では狩れない大きな獲物も狩れる() {
        let mut w = hunting_world(1.6);
        w.build_grid();
        assert_eq!(w.hunt_chance(0, 1), 0.0, "1 体では狩れない大きさ");
        let ally = genome::initial(false, 1.0, 1.0, 40.0, 0.5);
        w.spawn(100.0, 106.0, CARNIVORE, 5.0, 0, &ally);
        w.spawn(100.0, 94.0, CARNIVORE, 5.0, 0, &ally);
        w.build_grid();
        assert!(w.hunt_chance(0, 1) > 0.0, "仲間 2 体がいれば狩れる");
    }

    #[test]
    fn 邪魔をするのは系統の違う肉食だけ() {
        let mut w = hunting_world(0.5);
        w.build_grid();
        let alone = w.hunt_chance(0, 1);
        // 系統の違う（標識色が遠い）肉食
        let rival = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 106.0, CARNIVORE, 5.0, 0, &rival);
        w.build_grid();
        assert!(w.hunt_chance(0, 1) < alone, "系統の違う肉食は邪魔をする");
        // 同じ系統の肉食に入れ替える
        w.genomes[2 * GENOME_LEN + genome::G_HUE] = 0.5;
        w.traits[2].hue = 0.5;
        assert!(w.hunt_chance(0, 1) >= alone, "同じ系統の肉食は邪魔をしない");
    }

    #[test]
    fn 仕留めた獲物は近くの仲間と分け合う() {
        let mut w = hunting_world(0.5);
        let ally = genome::initial(false, 1.0, 1.0, 40.0, 0.5);
        w.spawn(100.0, 106.0, CARNIVORE, 5.0, 0, &ally);
        w.build_grid();
        w.params.hunt_success = 1.0;
        let before = w.energy[2];
        // 確実に成功させるため、成功率 1 で大きさの不利もない状態で何度か試す
        for _ in 0..20 {
            w.cooldown[0] = 0;
            if let Hunt::Caught = w.try_hunt(0, 1, 16.0) {
                assert!(w.energy[2] > before, "仲間も獲物を食べる");
                return;
            }
        }
        panic!("狩りに成功しなかった");
    }

    #[test]
    fn 仲間に囲まれた獲物は狩りにくい() {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        let herb = genome::initial(true, 0.5, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        w.spawn(105.0, 100.0, HERBIVORE, 5.0, 0, &herb);
        w.build_grid();
        let alone = w.hunt_chance(0, 1);
        for k in 0..4 {
            w.spawn(105.0 + (k as f32 - 1.5) * 4.0, 104.0, HERBIVORE, 5.0, 0, &herb);
        }
        w.build_grid();
        let guarded = w.hunt_chance(0, 1);
        assert!(guarded < alone * 0.7, "囲まれていると下がる: 単独 {alone} / 囲まれ {guarded}");
    }

    #[test]
    fn 子は小さく生まれて成熟するまで繁殖しない() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &g);
        face(&mut w, 0, 1.0, 0.0); // 相手が視野の中に入るように
        w.step();
        assert_eq!(w.count(), 3, "親が繁殖する");
        let child = (0..w.count()).find(|&i| w.generation[i] == 1).unwrap();
        assert!(w.growth[child] < 1.0, "子は成長途中で生まれる");
        assert!(w.traits[child].size < w.traits[child].adult_size, "子は大人より小さい");

        // エネルギーを十分に与えても、成熟するまでは繁殖しない
        let id = w.id[child];
        for _ in 0..50 {
            let c = w.index_of(id).unwrap();
            w.energy[c] = w.energy_cap(c);
            w.step();
            let c = w.index_of(id).unwrap();
            assert_eq!(w.children[c], 0, "幼体が繁殖した");
        }
    }

    #[test]
    fn 遺伝的に遠い相手とは繁殖できない() {
        let mut w = empty_world();
        let a = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let mut b = a.clone();
        // 脳の重みを大きくずらして、遺伝的に遠い相手にする
        for v in &mut b[genome::N_TRAITS..] {
            *v += 1.0;
        }
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &a);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &b);
        face(&mut w, 0, 1.0, 0.0);
        assert!(w.mating_chance(0, 1) < 1e-6, "相性はほぼ 0 のはず");
        w.step();
        assert_eq!(w.count(), 2, "遠い相手と繁殖した");
        assert_eq!(w.failed_matings, 1);
        assert!(w.energy[0] < 15.0, "失敗しても繁殖のコストは払う");
    }

    #[test]
    fn 繁殖できる個体は相手へ近づく() {
        let mut w = empty_world();
        // 草があると隠れの効果で相手が見えなくなるので、取り除く
        w.plants.fill(0.0);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        // 相手は繁殖できる距離（12px）の外、視野（30px）の内側
        w.spawn(100.0, 100.0, HERBIVORE, 15.0, 0, &g);
        w.spawn(125.0, 100.0, HERBIVORE, 2.0, 0, &g);
        face(&mut w, 0, 1.0, 0.0);
        let (a, b) = (w.id[0], w.id[1]);
        let dist = |w: &World| {
            let (i, j) = (w.index_of(a).unwrap(), w.index_of(b).unwrap());
            (torus_delta(w.xs[j] - w.xs[i], w.width).powi(2) + torus_delta(w.ys[j] - w.ys[i], w.height).powi(2)).sqrt()
        };
        let before = dist(&w);
        for _ in 0..15 {
            w.step();
            if w.count() > 2 {
                return; // 近づいて繁殖できた
            }
        }
        assert!(dist(&w) < before, "相手へ近づいていない: {before} → {}", dist(&w));
    }

    #[test]
    fn 地形のある世界で個体は水に入らず帳尻も合う() {
        let mut p = small_params();
        p.water_fraction = 0.3;
        p.fertility_min = 0.3;
        p.elevation_scale = 300.0;
        let mut w = World::new(4, p);
        assert!(w.terrain.land_cells < w.terrain.water.len(), "水域がある");
        for _ in 0..1000 {
            w.step();
            for i in 0..w.count() {
                assert!(!w.terrain.is_water(w.xs[i], w.ys[i]), "個体 {i} が水の上にいる");
            }
        }
        let rel = (w.ledger.expected_total() - w.total_energy()).abs() / w.ledger.expected_total();
        assert!(rel < 1e-4, "帳尻の相対誤差 {rel}");
        // 水のマスに植物は育たない
        for (k, &water) in w.terrain.water.iter().enumerate() {
            if water {
                assert_eq!(w.plants.biomass[k], 0.0);
            }
        }
    }

    #[test]
    fn 親子は互いの方向がわかり_他人には反応しない() {
        let mut w = empty_world();
        let g = genome::initial(false, 1.0, 1.8, 48.0, 0.3);
        // 0 が親、1 が 0 の子（成長途中）、2 は他人。全員が互いの正面に入るよう、東向きに並べて 0 と 1 は向かい合わせる
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        w.spawn(110.0, 100.0, CARNIVORE, 5.0, 0, &g);
        w.spawn(105.0, 103.0, CARNIVORE, 5.0, 0, &g);
        for (i, h) in [(0, (1.0, 0.0)), (1, (-1.0, 0.0)), (2, (0.0, -1.0))] {
            w.hx[i] = h.0;
            w.hy[i] = h.1;
        }
        w.parents[1] = [w.id[0], 0];
        w.growth[1] = 0.5;
        let inputs = w.brain_inputs();
        assert!(inputs[0][genome::IN_FAMILY_NEAR] > 0.5 && inputs[0][genome::IN_FAMILY_FWD] > 0.9, "親から子が見える");
        assert!(inputs[1][genome::IN_FAMILY_NEAR] > 0.5 && inputs[1][genome::IN_FAMILY_FWD] > 0.9, "子から親が見える");
        assert_eq!(inputs[2][genome::IN_FAMILY_NEAR], 0.0, "他人には親子がいない");
        assert_eq!(inputs[1][genome::IN_GROWTH], 0.5);
        assert_eq!(inputs[0][genome::IN_GROWTH], 1.0);
    }

    /// 成長度 growth の個体を加え、その番号を返す（テスト用）
    fn spawn_young(w: &mut World, x: f32, y: f32, kind: u8, energy: f32, g: &[f32], growth: f32) -> usize {
        let t = genome::traits(g, w.params.max_turn);
        w.spawn_grown(x, y, kind, energy, 0, g, t, growth);
        w.count() - 1
    }

    #[test]
    fn 未熟な子は動けず自分では食べない() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.3, 30.0, 0.0);
        let small = spawn_young(&mut w, 100.0, 100.0, HERBIVORE, 0.2, &g, 0.1);
        let big = spawn_young(&mut w, 300.0, 300.0, HERBIVORE, 0.2, &g, 0.5);
        assert_eq!(w.mobility(small), 0.0);
        assert_eq!(w.mobility(big), 1.0);
        let mid = spawn_young(&mut w, 500.0, 500.0, HERBIVORE, 0.2, &g, 0.3);
        assert!((w.mobility(mid) - 0.5).abs() < 1e-6, "中間はなめらか");
        let (small_id, big_id) = (w.id[small], w.id[big]);
        let (x0, y0, e0) = (w.xs[small], w.ys[small], w.energy[small]);
        let big_e0 = w.energy[big];
        for _ in 0..20 {
            w.step();
        }
        let (s, b) = (w.index_of(small_id).unwrap(), w.index_of(big_id).unwrap());
        assert_eq!((w.xs[s], w.ys[s]), (x0, y0), "動けない");
        assert!(w.energy[s] < e0, "自分では食べられない: {} → {}", e0, w.energy[s]);
        assert!(w.energy[b] > big_e0, "動ける子は食べる: {} → {}", big_e0, w.energy[b]);
    }

    #[test]
    fn 親は届く距離の自分の幼い子に与え_帳簿は合う() {
        let mut w = empty_world();
        let mut g = genome::initial(false, 1.0, 1.8, 48.0, 0.0);
        genome::add_feed_wiring(&mut g, 4.0);
        w.spawn(100.0, 100.0, CARNIVORE, 16.0, 0, &g);
        let child = spawn_young(&mut w, 103.0, 100.0, CARNIVORE, 0.2, &g, 0.1);
        let stranger = spawn_young(&mut w, 97.0, 100.0, CARNIVORE, 0.2, &g, 0.1);
        w.parents[child] = [w.id[0], 0];
        w.ledger.initial = w.total_energy();
        let (parent_id, child_id, stranger_id) = (w.id[0], w.id[child], w.id[stranger]);
        let inputs = w.brain_inputs();
        assert!(inputs[0][genome::IN_CHILD_NEAR] > 0.9 && inputs[0][genome::IN_CHILD_HUNGER] > 0.9, "親は自分の子に気づく");
        assert_eq!(inputs[child][genome::IN_CHILD_NEAR], 0.0, "子には子がいない");
        for _ in 0..10 {
            w.step();
        }
        let (p, c, s) = (w.index_of(parent_id).unwrap(), w.index_of(child_id).unwrap(), w.index_of(stranger_id).unwrap());
        assert!(w.energy[c] > 0.2 + 0.05, "子は与えられて蓄えが増える: {}", w.energy[c]);
        assert!(w.energy[s] < 0.2, "他人の子には与えない: {}", w.energy[s]);
        assert!(w.energy[p] < 16.0);
        assert!(w.census.feeds[CARNIVORE as usize] > 0);
        let gap = (w.total_energy() - w.ledger.expected_total()).abs();
        assert!(gap < 1e-3, "帳簿が合う（差 {gap}）");
    }

    #[test]
    fn 産んだ場所の方向を子が育つまで覚えている() {
        let mut w = empty_world();
        let g = genome::initial(false, 1.0, 1.8, 48.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        (w.hx[0], w.hy[0]) = (1.0, 0.0);
        assert_eq!(w.brain_inputs()[0][genome::IN_DEN_FWD], 0.0, "産んでいなければ覚えていない");
        w.den[0] = (150.0, 100.0);
        w.den_until[0] = w.tick + 100;
        assert!(w.brain_inputs()[0][genome::IN_DEN_FWD] > 0.99, "前方にある");
        w.den_until[0] = w.tick;
        assert_eq!(w.brain_inputs()[0][genome::IN_DEN_FWD], 0.0, "子が育ったら忘れる");
    }

    #[test]
    fn 近づきすぎた個体どうしは離れる() {
        // 離れる力がある場合とない場合で、同じ条件から動かした後の距離を比べる（ほかの欲求の影響を打ち消す）
        let distance = |weight: f32| {
            let mut w = empty_world();
            w.params.separation_weight = weight;
            w.plants.fill(0.0);
            let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
            w.spawn(101.5, 100.0, HERBIVORE, 5.0, 0, &g);
            // 離れたあと、仲間どうしで寄り戻ることがあるので、最後の距離ではなく 20 tick のあいだの最大の距離で比べる
            let mut far = 0.0f32;
            for _ in 0..20 {
                w.step();
                far = far.max((torus_delta(w.xs[1] - w.xs[0], w.width).powi(2) + torus_delta(w.ys[1] - w.ys[0], w.height).powi(2)).sqrt());
            }
            far
        };
        let (with, without) = (distance(1.5), distance(0.0));
        assert!(with > without + 1.0, "離れる力あり {with} / なし {without}");
    }

    #[test]
    fn 大きな獲物を襲って失敗すると反撃を受ける() {
        let mut w = hunting_world(2.0);
        w.params.hunt_success = 0.0; // 必ず失敗させる
        w.build_grid();
        let id = w.id[0];
        assert!(matches!(w.try_hunt(0, 1, 16.0), Hunt::Failed));
        // けが 8 × (2.0 − 1.2) = 6.4 は、手持ち 5 − 襲う費用 0.3 を超えるので死ぬ
        assert!(!w.alive[0], "反撃で死ぬはず");
        assert_eq!(w.id[0], id);
        assert_eq!(w.watch.death, None);
    }

    #[test]
    fn 襲う費用は自分の大きさに比例する() {
        let spent = |size: f32| {
            let mut w = empty_world();
            w.params.hunt_success = 0.0;
            let carn = genome::initial(false, size, 1.0, 40.0, 0.5);
            w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
            let herb = genome::initial(true, 0.1, 1.0, 30.0, 0.0);
            w.spawn(101.0, 100.0, HERBIVORE, 5.0, 0, &herb);
            w.build_grid();
            w.try_hunt(0, 1, 1.0);
            (5.0 - w.energy[0]) / w.traits[0].size
        };
        assert!((spent(0.5) - spent(2.0)).abs() < 1e-4, "大きさあたりの費用は同じ");
    }

    #[test]
    fn 小さな獲物なら失敗しても反撃を受けない() {
        let mut w = hunting_world(1.0);
        w.params.hunt_success = 0.0;
        w.build_grid();
        w.try_hunt(0, 1, 16.0);
        assert!((w.energy[0] - (5.0 - w.params.attack_cost * w.traits[0].size)).abs() < 1e-5, "襲う費用だけ失う");
    }

    #[test]
    fn 生まれた場所と大きく違う植生の草は食べられない() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 2.0, 0, &g);
        // 平らな世界の植生は 1。生まれた植生が近ければ食べられる
        w.natal[0] = 0.9;
        assert!(w.graze(0), "植生の差 0.1 なら食べられる");
        w.natal[0] = 0.5;
        assert!(!w.graze(0), "植生の差 0.5 では食べられない");
        assert_eq!(w.habitat_fit(0, 1.0), 0.0);
    }

    #[test]
    fn 止まっている個体は消費が少ない() {
        let mut w = empty_world();
        w.params.move_cost = 0.0; // 基本の代謝の違いだけを見る
        let g = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 10.0, 0, &g);
        w.spawn(300.0, 300.0, CARNIVORE, 10.0, 0, &g);
        w.live(0, 0.0, false, Mate::Missing);
        w.live(1, 1.0, false, Mate::Missing);
        assert!(w.energy[0] > w.energy[1], "止まっているほうが消費が少ない");
    }

    #[test]
    fn 見つかる距離は大きさの3分の1乗に比例し草の隠れは大きさによらない() {
        let mut w = empty_world();
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 0.5, 1.0, 30.0, 0.0));
        w.spawn(200.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 1.0, 1.0, 30.0, 0.0));
        w.spawn(300.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 2.0, 1.0, 30.0, 0.0));
        let visible = |w: &World, i: usize| 1.0 - w.conceal[i];
        // 草のない場所
        w.cover = vec![0.0; 3];
        w.build_grid();
        assert!((visible(&w, 0) - 0.5f32.powf(1.0 / 3.0)).abs() < 1e-3, "小さいと見つかる距離が縮む");
        assert!((visible(&w, 1) - 1.0).abs() < 1e-6, "大きさ 1 は等倍");
        assert!((visible(&w, 2) - 1.0).abs() < 1e-6, "見る側の視野は超えない");
        // 草が満杯の場所: 大きさ 1 は (1 − 隠れの強さ) 倍、大きさ 0.5 はさらに大きさの分だけ縮む（掛け算）
        w.cover = vec![1.0; 3];
        w.build_grid();
        let open = 1.0 - w.params.cover_strength;
        assert!((visible(&w, 1) - open).abs() < 1e-5);
        assert!((visible(&w, 0) / visible(&w, 1) - 0.5f32.powf(1.0 / 3.0)).abs() < 1e-3, "草の中でも大きさの差は同じ比");
    }

    #[test]
    fn 移動の消費は大きさの0_7乗に比例し体重あたりは大きいほど割安() {
        // 動いたときと止まっているときの消費の差（移動の分だけ）を測る。基本の代謝は 0 にしておく
        let moving = |size: f32| {
            let run = |speed: f32| {
                let mut w = empty_world();
                w.params.herb_metabolism = 0.0;
                w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, size, 2.0, 30.0, 0.0));
                w.live(0, speed, false, Mate::Missing);
                5.0 - w.energy[0]
            };
            run(1.0) - run(0.0)
        };
        let (unit, big) = (moving(1.0), moving(2.0));
        assert!((unit - empty_world().params.move_cost).abs() < 1e-6, "大きさ 1・速さ 1 で move_cost");
        assert!((big / unit - 2.0f32.powf(0.7)).abs() < 1e-3, "大きさの 0.7 乗: {big} / {unit}");
    }

    #[test]
    fn 草食の消化の効率は大人の大きさの4分の1乗に比例する() {
        let mut w = empty_world();
        for (k, size) in [0.5f32, 1.0, 2.0, 40.0].iter().enumerate() {
            w.spawn(100.0 * (k + 1) as f32, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, *size, 1.0, 30.0, 0.0));
        }
        let base = w.params.herb_efficiency;
        assert!((w.digest_efficiency(1) - base).abs() < 1e-6, "大人の大きさ 1 は基準どおり");
        assert!((w.digest_efficiency(0) / base - 0.5f32.powf(0.25)).abs() < 1e-4);
        assert!((w.digest_efficiency(2) / base - 2.0f32.powf(0.25)).abs() < 1e-4);
        assert!(w.digest_efficiency(3) <= 0.95, "1 を超えるとエネルギーが湧くので上限を設ける");
    }

    #[test]
    fn 肉食は好みの大きさの獲物だけを狙う() {
        let mut w = empty_world();
        w.plants.fill(0.0);
        // 好みの下限は 0.25 なので、肉食を 2.0 にして小さい獲物（0.2）との比を範囲外（0.1）にする
        let carn = genome::initial(false, 2.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        // 25px 先に小さすぎる獲物（比 0.1。気づく距離は 40 × 0.2^(1/3) × 0.035 ≈ 0.8px まで縮む）、30px 先に好みの大きさの獲物（比 0.4）
        w.spawn(125.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 0.2, 1.0, 30.0, 0.0));
        w.spawn(130.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 0.8, 1.0, 30.0, 0.0));
        face(&mut w, 0, 1.0, 0.0);
        w.build_grid();
        let prey = w.sense(0).prey.map(|(j, ..)| j);
        assert_eq!(prey, Some(2), "少し離れた小さすぎる獲物は気にせず、好みの大きさの獲物を狙う");
        // 好みの獲物がいなければ、目の前の小さすぎる獲物も狙う（小さい獲物は見つかる距離も 0.2^(1/3) 倍に縮むので、すぐ目の前に置く）
        w.xs[1] = 100.5;
        w.xs[2] = 300.0;
        w.build_grid();
        let prey = w.sense(0).prey.map(|(j, ..)| j);
        assert_eq!(prey, Some(1), "目の前の小さな獲物は狙う");
    }

    #[test]
    fn 好みの気温から離れた場所では消費が多い() {
        let mut w = empty_world();
        let mut warm = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        warm[genome::G_TEMP_PREF] = w.params.temp_mean;
        let mut cold = warm.clone();
        cold[genome::G_TEMP_PREF] = w.params.temp_mean - 20.0;
        w.spawn(100.0, 100.0, HERBIVORE, 10.0, 0, &warm);
        w.spawn(300.0, 300.0, HERBIVORE, 10.0, 0, &cold);
        w.live(0, 0.0, false, Mate::Missing);
        w.live(1, 0.0, false, Mate::Missing);
        assert!(w.energy[0] > w.energy[1], "好みの気温と合わない個体のほうが消費が多い");
    }

    #[test]
    fn 全力で走ると体力が尽き_巡航の速さまでしか出せなくなる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.3, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 1000.0, 0, &g);
        let max = w.traits[0].max_speed;
        assert_eq!(w.stamina[0], 1.0, "生まれたときは体力が満ちている");
        assert_eq!(w.stamina_cap(0), 1.0);
        let sprint = w.params.stamina_sprint as usize;
        for _ in 0..sprint / 2 {
            w.live(0, max, false, Mate::Missing);
        }
        assert!((w.stamina[0] - 0.5).abs() < 0.02, "全力なら stamina_sprint の半分で半分減る: {}", w.stamina[0]);
        assert_eq!(w.stamina_cap(0), 1.0, "まだ全速が出る");
        for _ in 0..sprint {
            w.live(0, max, false, Mate::Missing);
        }
        assert_eq!(w.stamina[0], 0.0);
        assert!((w.stamina_cap(0) - w.params.herb_cruise).abs() < 1e-6, "尽きると巡航の速さまで");
        // 巡航の速さちょうどなら減りも戻りもしない
        w.stamina[0] = 0.5;
        w.live(0, max * w.params.herb_cruise, false, Mate::Missing);
        assert!((w.stamina[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn 止まっていれば体力が戻り_疲れると巡航の速さまで落ちる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.3, 30.0, 0.0);
        let c = genome::initial(false, 1.0, 1.8, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 1000.0, 0, &g);
        w.spawn(300.0, 300.0, CARNIVORE, 1000.0, 0, &c);
        w.stamina[0] = 0.0;
        for _ in 0..(w.params.stamina_recover as usize / 2) {
            w.live(0, 0.0, false, Mate::Missing);
        }
        assert!((w.stamina[0] - 0.5).abs() < 0.02, "止まっていれば stamina_recover の半分で半分戻る: {}", w.stamina[0]);
        w.stamina[1] = 0.0;
        let tired = w.stamina_cap(1) * w.traits[1].max_speed;
        assert!((tired - w.params.carn_cruise * w.traits[1].max_speed).abs() < 1e-5);
        // 疲れた肉食（最高速度 1.8）は、元気に逃げる草食（最高速度 1.3）より遅い
        w.stamina[0] = 1.0;
        assert!(tired < w.stamina_cap(0) * w.traits[0].max_speed);
        // 体力は脳の入力に入る
        w.stamina[0] = 0.3;
        w.build_grid();
        w.build_calls();
        assert!((w.brain_inputs()[0][genome::IN_STAMINA] - 0.3).abs() < 1e-6);
    }


    #[test]
    fn 飢えた個体と老いた個体は遅くなる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.3, 30.0, 0.0);
        let need = w.params.herb_repro_threshold;
        w.spawn(100.0, 100.0, HERBIVORE, need, 0, &g);
        assert_eq!(w.vigor(0), 1.0, "元気な大人は全速が出る");
        w.energy[0] = 0.0;
        assert!((w.vigor(0) - w.params.frail_speed).abs() < 1e-6, "飢えきると最も遅い");
        w.energy[0] = need * w.params.frail_energy * 0.5;
        let half_starved = w.vigor(0);
        assert!(half_starved > w.params.frail_speed && half_starved < 1.0);
        w.energy[0] = need;
        w.age[0] = (w.params.herb_max_age * 0.95) as u32;
        assert!(w.vigor(0) < 0.7, "寿命の近い個体は遅い: {}", w.vigor(0));
    }

    #[test]
    fn 日射は季節で増減する() {
        let grown = |tick: u64| {
            let mut p = small_params();
            p.herb_initial = 0.0;
            p.carn_initial = 0.0;
            p.season_amplitude = 0.4;
            let mut w = World::new(1, p);
            w.tick = tick;
            w.step();
            w.ledger.sunlight_in
        };
        let len = Params::default().season_length as u64;
        let (summer, winter) = (grown(len / 4), grown(len * 3 / 4));
        assert!(summer > winter * 2.0, "夏 {summer} 冬 {winter}");
    }

    #[test]
    fn 繁殖シグナルを出すと草に隠れにくい() {
        let mut w = empty_world();
        let g = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(130.0, 100.0, HERBIVORE, 5.0, 0, &g);
        face(&mut w, 0, 1.0, 0.0);
        w.params.cover_strength = 0.6;
        w.params.signal_exposure = 1.0;
        w.cover = vec![0.0, 1.0];
        w.build_grid();
        assert!(w.sense(0).prey.is_none(), "シグナルを出していなければ草むらに隠れる");
        w.signal[1] = 1.0;
        w.build_grid();
        assert!(w.sense(0).prey.is_some(), "シグナルを出すと見つかる");
    }

    #[test]
    fn 仲間の繁殖シグナルが見える() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
        w.spawn(115.0, 100.0, HERBIVORE, 5.0, 0, &g);
        w.spawn(100.0, 115.0, HERBIVORE, 5.0, 0, &g);
        w.signal = vec![0.0, 0.8, 0.4];
        face(&mut w, 0, 1.0, 1.0);
        w.build_grid();
        let s = w.sense(0);
        assert_eq!(s.kin_count, 2);
        assert!((s.kin_signal - 1.2).abs() < 1e-5);
    }

    #[test]
    fn 速く走っているほど曲がれない() {
        let mut w = empty_world();
        let g = genome::initial(false, 1.0, 2.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &g);
        let angle = |(c, s): (f32, f32)| s.atan2(c);
        let (still, walk, run) = (angle(w.turn_limit(0, 0.0)), angle(w.turn_limit(0, 0.5)), angle(w.turn_limit(0, 2.0)));
        assert!((still - w.params.max_turn).abs() < 1e-5, "止まっていれば体の上限まで曲がれる: {still}");
        assert_eq!(still, walk, "ゆっくりなら制限されない");
        assert!((run - w.params.turn_accel / 2.0).abs() < 1e-5, "全速では 横の加速度 ÷ 速さ: {run}");
    }

    #[test]
    fn 繁殖したあとしばらくは繁殖できない() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 30.0, 0, &g);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &g);
        assert!(w.can_breed(0));
        w.live(0, 0.0, true, Mate::Partner(1));
        assert_eq!(w.births.len(), 1);
        w.energy[0] = 30.0;
        assert!(!w.can_breed(0), "繁殖した直後は休む");
        w.live(0, 0.0, true, Mate::Partner(1));
        assert_eq!(w.births.len(), 1, "休んでいる間は繁殖しない");
    }

    #[test]
    fn 寿命は個体ごとにばらつく() {
        let w = World::new(1, small_params());
        let spans: Vec<f32> = (0..w.count()).filter(|&i| w.kind[i] == HERBIVORE).map(|i| w.lifespan(i)).collect();
        let (min, max) = spans.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let base = w.params.herb_max_age;
        assert!(min >= base * 0.75 - 1.0 && max <= base * 1.25 + 1.0, "範囲 {min}〜{max}");
        assert!(max - min > base * 0.4, "十分にばらつく: {min}〜{max}");
    }

    #[test]
    fn 季節で寒い地域の植物の育ちやすさが変わる() {
        let mut p = small_params();
        p.water_fraction = 0.25;
        p.fertility_min = 0.3;
        p.world_width = 2400.0;
        p.world_height = 1600.0;
        p.herb_initial = 0.0;
        p.carn_initial = 0.0;
        let mut w = World::new(1, p);
        let edge = |w: &World| (0..w.plants.cols).map(|c| w.plants.fertility[c]).sum::<f32>();
        let len = w.params.season_length as u64;
        w.tick = len / 4;
        w.update_climate();
        let summer = edge(&w);
        w.tick = len * 3 / 4;
        w.update_climate();
        let winter = edge(&w);
        assert!(summer > winter * 1.5, "夏 {summer} 冬 {winter}");
        // 植生の種類（草食が覚えるもの）は季節で変わらない
        assert_eq!(w.plants.vegetation, w.terrain.soil);
    }

    #[test]
    fn 視野角の外にいる相手は見えない() {
        let mut w = empty_world();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0); // 視野角 120°
        w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        w.spawn(120.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 1.0, 1.0, 30.0, 0.0));
        w.cover = vec![0.0; 2];
        w.build_grid();
        face(&mut w, 0, 1.0, 0.0);
        assert!(w.sense(0).prey.is_some(), "正面の獲物は見える");
        face(&mut w, 0, 0.0, 1.0);
        assert!(w.sense(0).prey.is_none(), "真横（90°）は視野角 120° の外");
        face(&mut w, 0, 1.0, 1.0);
        assert!(w.sense(0).prey.is_some(), "斜め前（45°）は見える");
        // 草食の視野角 300° なら、真後ろ以外は見える
        let mut w = empty_world();
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 1.0, 1.0, 30.0, 0.0));
        w.spawn(120.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        w.vigilance[0] = 1.0;
        w.cover = vec![0.0; 2];
        w.build_grid();
        face(&mut w, 0, 0.0, 1.0);
        assert!(w.sense(0).predator.is_some(), "真横の天敵は見える");
        face(&mut w, 0, -1.0, 0.0);
        assert!(w.sense(0).predator.is_none(), "真後ろの天敵は見えない");
    }

    #[test]
    fn 動いている相手ほど草に紛れず_警戒していない草食も走ってくる天敵には気づく() {
        // 草の満ちたマスにいる肉食: 止まっていれば隠れ、走っていれば見つかる
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        let herb = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let mut w = empty_world();
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &herb);
        w.spawn(118.0, 100.0, CARNIVORE, 5.0, 0, &carn);
        w.hx[0] = 1.0;
        w.hy[0] = 0.0;
        w.vigilance[0] = 1.0;
        w.cover = vec![0.0, 1.0];
        w.build_grid();
        let hidden = w.conceal[1];
        assert!(w.sense(0).predator.is_none(), "草に紛れて止まっている天敵は 18 px 先でも見えない");
        let max = w.traits[1].max_speed;
        w.vel[1] = (0.0, max);
        w.build_grid();
        assert!((w.motion[1] - 1.0).abs() < 1e-6);
        assert!(w.conceal[1] < hidden - 0.5, "全力で走ると草に紛れない: {hidden} → {}", w.conceal[1]);
        assert!(w.sense(0).predator.is_some(), "走っている天敵は見える");
        // 草のない場所: 警戒していない草食は、止まっている天敵には視野の半分までしか気づかないが、走ってくる天敵には視野いっぱいで気づく
        w.cover = vec![0.0, 0.0];
        w.vigilance[0] = 0.0;
        w.xs[1] = 100.0 + 0.7 * w.traits[0].vision;
        w.vel[1] = (0.0, 0.0);
        w.build_grid();
        assert!(w.sense(0).predator.is_none(), "止まっている天敵には気づかない");
        w.vel[1] = (0.0, max);
        w.build_grid();
        assert!(w.sense(0).predator.is_some(), "走ってくる天敵には気づく");
    }

    #[test]
    fn 視野は広く遠いほど目の維持費がかかる() {
        let cost = |fov_deg: f32, vision: f32| {
            let mut w = empty_world();
            w.params.move_cost = 0.0;
            let mut g = genome::initial(true, 1.0, 1.0, vision, 0.0);
            g[genome::G_FOV] = fov_deg.to_radians();
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
            w.live(0, 0.0, false, Mate::Missing);
            5.0 - w.energy[0]
        };
        assert!(cost(300.0, 30.0) > cost(120.0, 30.0), "広いほど高い");
        assert!(cost(120.0, 48.0) > cost(120.0, 30.0), "遠いほど高い");
    }

    #[test]
    fn 視野の外の呼び声が聞こえる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let cell = w.params.call_cell;
        w.spawn(cell * 1.5, cell * 1.5, HERBIVORE, 5.0, 0, &g);
        // 右隣のマスで、視野（30px）より遠い個体が呼んでいる
        w.spawn(cell * 2.5, cell * 1.5, HERBIVORE, 5.0, 0, &g);
        w.signal = vec![0.0, 1.0];
        w.build_calls();
        let (dx, dy, total) = w.hear_calls(0);
        assert!(dx > 0.0 && dy.abs() < 1e-6 && total > 0.9, "右から聞こえる: {dx} {dy} {total}");
        // 自分の声は聞こえない
        let (_, _, own) = w.hear_calls(1);
        assert!(own < 1e-6, "自分の声 {own}");
    }

    #[test]
    fn 背後を見ている仲間を数える() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0); // 視野角 300°
        let mut narrow = g.clone();
        narrow[genome::G_FOV] = 90f32.to_radians();
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
        w.spawn(110.0, 100.0, HERBIVORE, 5.0, 0, &narrow);
        w.spawn(100.0, 110.0, HERBIVORE, 5.0, 0, &narrow);
        w.cover = vec![0.0; 3];
        face(&mut w, 0, 1.0, 1.0);
        // 仲間 1 は自分と反対（後ろ）を向き、仲間 2 は同じ向き
        face(&mut w, 1, -1.0, -1.0);
        face(&mut w, 2, 1.0, 1.0);
        w.build_grid();
        let s = w.sense(0);
        assert_eq!(s.kin_count, 2);
        assert_eq!(s.rear_watchers, 1, "後ろを向いた仲間だけが背後を見ている");
    }

    #[test]
    fn 寿命で死んだ個体は死体になり肉食が食べられる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 6.0, 0, &g);
        w.age[0] = 100_000;
        w.live(0, 0.0, false, Mate::Missing);
        let c = w.plants.cell_index(100.0, 100.0);
        assert!(w.carrion[c] > 5.0, "残っていたエネルギーが死体になる: {}", w.carrion[c]);
        w.remove_dead();
        let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 2.0, 0, &carn);
        let before = w.carrion[c];
        assert!(w.scavenge(0));
        assert!(w.carrion[c] < before && w.energy[0] > 2.0, "肉食が死体を食べる");
    }

    #[test]
    fn 死体は腐って減り帳尻は合う() {
        let mut w = empty_world();
        let c = w.plants.cell_index(100.0, 100.0);
        w.ledger.initial = w.total_energy();
        // 帳簿の外から足すので、開始時の量にも足しておく
        w.drop_carrion(100.0, 100.0, 10.0);
        w.ledger.initial += 10.0;
        for _ in 0..70 {
            w.step();
        }
        assert!(w.carrion[c] < 6.0 && w.carrion[c] > 3.0, "半減期くらいで半分ほどに: {}", w.carrion[c]);
        for _ in 0..500 {
            w.step();
        }
        assert_eq!(w.carrion[c], 0.0, "少なくなった死体は消える");
        let err = (w.ledger.expected_total() - w.total_energy()).abs();
        assert!(err < 1e-3, "帳尻の誤差 {err}");
    }

    #[test]
    fn 系統の統計は標識色の範囲の個体だけを数える() {
        let mut w = empty_world();
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        for (x, hue) in [(100.0, 0.02), (200.0, 0.98), (300.0, 0.5)] {
            g[genome::G_HUE] = hue;
            w.spawn(x, 100.0, HERBIVORE, 5.0, 0, &g);
        }
        // 0.95〜0.05（1 をまたぐ範囲）
        let s = w.lineage_stats(HERBIVORE, 0.95, 0.1);
        assert_eq!(s[0], 2.0);
        assert!((s[10] - 150.0).abs() < 1.0, "重心の x {}", s[10]);
        assert_eq!(w.lineage_stats(HERBIVORE, 0.0, 1.0)[0], 3.0, "幅 1 なら全体");
        assert_eq!(w.lineage_stats(CARNIVORE, 0.0, 1.0)[0], 0.0);
    }

    #[test]
    fn 子は両親の_id_を持ち生涯の記録が残る() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 30.0, 0, &g);
        w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &g);
        let (a, b) = (w.id[0], w.id[1]);
        w.live(0, 1.0, true, Mate::Partner(1));
        w.apply_births();
        assert_eq!(w.parents[2], [a, b]);
        assert_eq!(w.living_children(a), 1);
        assert_eq!(w.living_children(b), 1);
        assert_eq!(w.life[0].matings, 1);
        w.move_agent(0, 1.5);
        assert!((w.life[0].distance - 1.5).abs() < 1e-6);
    }

    #[test]
    fn 古い死体は栄養が少なく病気になりやすい() {
        let eat = |age: f32, seed: u64| {
            let mut p = small_params();
            p.herb_initial = 0.0;
            p.carn_initial = 0.0;
            p.sunlight = 0.0;
            let mut w = World::new(seed, p);
            let carn = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
            w.spawn(100.0, 100.0, CARNIVORE, 5.0, 0, &carn);
            w.drop_carrion(100.0, 100.0, 20.0);
            let c = w.plants.cell_index(100.0, 100.0);
            w.carrion_age[c] = age;
            w.build_grid();
            let before = w.energy[0];
            w.scavenge(0);
            w.energy[0] - before
        };
        // 新鮮な死体は病気にならず、栄養がある
        let fresh = eat(0.0, 1);
        assert!(fresh > 0.05, "新鮮な死体の栄養 {fresh}");
        // 腐りきった死体はほとんど栄養がなく、何度も食べると病気でエネルギーを失うことがある
        let rotten: Vec<f32> = (0..200).map(|s| eat(2000.0, s)).collect();
        assert!(rotten.iter().cloned().fold(f32::MIN, f32::max) < fresh * 0.1, "腐った死体の栄養は少ない");
        assert!(rotten.iter().any(|&d| d < -1.0), "腐った死体で病気になることがある");
    }

    #[test]
    fn 新しい死体が足されると古さは量で重み付けして平均される() {
        let mut w = empty_world();
        let c = w.plants.cell_index(100.0, 100.0);
        w.drop_carrion(100.0, 100.0, 10.0);
        w.carrion_age[c] = 100.0;
        w.drop_carrion(100.0, 100.0, 10.0);
        assert!((w.carrion_age[c] - 50.0).abs() < 1e-3, "古さ {}", w.carrion_age[c]);
    }

    #[test]
    fn 系統の中からランダムに選べる() {
        let mut w = empty_world();
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        for (x, hue) in [(100.0, 0.1), (200.0, 0.12), (300.0, 0.5)] {
            g[genome::G_HUE] = hue;
            w.spawn(x, 100.0, HERBIVORE, 5.0, 0, &g);
        }
        let picked: std::collections::HashSet<u32> = (0..50).map(|n| w.random_in_lineage(HERBIVORE, 0.05, 0.1, n)).collect();
        assert_eq!(picked, [w.id[0], w.id[1]].into_iter().collect(), "範囲内の 2 体だけが、どちらも選ばれる");
        assert_eq!(w.random_in_lineage(CARNIVORE, 0.0, 1.0, 0), 0);
    }

    #[test]
    fn 餓死した個体も体が死体になる() {
        let mut w = empty_world();
        let g = genome::initial(false, 1.0, 1.0, 40.0, 0.0);
        w.spawn(100.0, 100.0, CARNIVORE, 0.001, 0, &g);
        let body = w.body_energy(0);
        assert!((body - w.params.body_energy).abs() < 1e-5, "大人の大きさ 1 の体は body_energy");
        w.live(0, 1.0, false, Mate::Missing);
        assert!(!w.alive[0], "餓死する");
        let c = w.plants.cell_index(100.0, 100.0);
        assert!((w.carrion[c] - body).abs() < 1e-4, "体が死体になる: {}", w.carrion[c]);
    }

    #[test]
    fn 育つには蓄えから体のエネルギーを払う() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let traits = genome::traits(&g, w.params.max_turn);
        w.spawn_grown(100.0, 100.0, HERBIVORE, 5.0, 1, &g, traits, 0.5);
        let total = w.energy[0] + w.body_energy(0);
        w.grow(0, 600.0);
        assert!(w.growth[0] > 0.5, "育つ");
        assert!((w.energy[0] + w.body_energy(0) - total).abs() < 1e-5, "蓄え + 体 は変わらない（蓄えが体に移る）");
        // 蓄えがなければ育たない
        w.energy[0] = 0.0;
        let before = w.growth[0];
        w.grow(0, 600.0);
        assert_eq!(w.growth[0], before);
    }

    #[test]
    fn 食べ物の場所を覚えて方向を入力に渡し記憶は薄れる() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
        face(&mut w, 0, 1.0, 0.0);
        w.cover = vec![0.0; 1];
        w.build_grid();
        assert_eq!(w.plan(0).input[genome::IN_MEM_VALUE], 0.0, "生まれたときは何も覚えていない");

        w.remember(0, 0.6);
        assert_eq!(w.memory[0].value, 0.6);
        // 覚えた場所にいれば方向は 0
        let input = w.plan(0).input;
        assert!((input[genome::IN_MEM_VALUE] - 0.6).abs() < 1e-6);
        assert_eq!((input[genome::IN_MEM_FWD], input[genome::IN_MEM_RIGHT]), (0.0, 0.0));

        // 離れると、覚えた場所（後ろ）の方向を指す
        w.xs[0] = 200.0;
        let input = w.plan(0).input;
        assert!(input[genome::IN_MEM_FWD] < -0.99, "後ろを指す: {}", input[genome::IN_MEM_FWD]);
        // 今いる場所が悪ければ覚え直さない
        w.remember(0, 0.3);
        assert_eq!(w.memory[0].x, 100.0);

        // 記憶の長さだけ経つと 1/e に薄れ、今の場所のほうが良ければ覚え直す
        w.tick += w.traits[0].memory as u64;
        let faded = w.recalled_value(0);
        assert!((faded - 0.6 / std::f32::consts::E).abs() < 1e-3, "薄れる: {faded}");
        w.remember(0, 0.3);
        assert_eq!((w.memory[0].x, w.memory[0].value), (200.0, 0.3));

        // 覚えた場所で食べて悪くなっても、来たときの良さを覚えたまま（記憶が足元についてこない）
        w.xs[0] = 203.0;
        w.remember(0, 0.1);
        assert_eq!((w.memory[0].x, w.memory[0].value), (200.0, 0.3));
        w.xs[0] = 300.0;
        assert!(w.plan(0).input[genome::IN_MEM_FWD] < -0.99, "食べ終えて離れると、来た場所を指す");
    }

    #[test]
    fn 記憶が長いほど維持費がかかる() {
        let cost = |memory: f32| {
            let mut w = empty_world();
            let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
            g[genome::G_MEMORY] = memory;
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
            w.cover = vec![0.0; 1];
            let before = w.energy[0];
            w.live(0, 0.0, false, Mate::Missing);
            before - w.energy[0]
        };
        let (short, long) = (cost(100.0), cost(5100.0));
        let expected = empty_world().params.memory_cost * 5.0;
        assert!((long - short - expected).abs() < 1e-6, "差 {} / 期待 {expected}", long - short);
    }

    #[test]
    fn 系統色の離れた同じ種類の個体を敵として見る() {
        let mut w = empty_world();
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
        g[genome::G_HUE] = 0.02;
        w.spawn(110.0, 100.0, HERBIVORE, 5.0, 0, &g); // 同じ系統
        g[genome::G_HUE] = 0.5;
        w.spawn(120.0, 100.0, HERBIVORE, 5.0, 0, &g); // 敵
        face(&mut w, 0, 1.0, 0.0);
        w.cover = vec![0.0; 3];
        w.build_grid();
        let s = w.sense(0);
        assert_eq!(s.rival_count, 1);
        assert_eq!(s.rival.map(|r| r.0), Some(2));
    }

    #[test]
    fn 争いは大きいほうが勝ちやすく負けた側はけがをする() {
        let mut wins = 0;
        for seed in 0..200 {
            let mut p = small_params();
            p.herb_initial = 0.0;
            p.carn_initial = 0.0;
            p.sunlight = 0.0;
            let mut w = World::new(seed, p);
            let mut big = genome::initial(true, 2.0, 1.0, 30.0, 0.0);
            big[genome::G_HUE] = 0.0;
            let mut small = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
            small[genome::G_HUE] = 0.5;
            w.spawn(100.0, 100.0, HERBIVORE, 20.0, 0, &big);
            w.spawn(104.0, 100.0, HERBIVORE, 20.0, 0, &small);
            w.cover = vec![0.0; 2];
            w.build_grid();
            w.fight(0, 1, death::FOUGHT);
            if w.attacked[1] > 0 {
                wins += 1;
                assert!(w.energy[1] < 20.0 - w.params.fight_damage * 1.9, "負けた側は大きなけが");
            }
        }
        // 強さ 2 対 1 なら 2 乗の比で 8 割
        assert!((140..=180).contains(&wins), "大きいほうの勝ち数 {wins}/200");
    }

    #[test]
    fn 争いで死んだ個体は死体になる() {
        let mut w = empty_world();
        let mut a = genome::initial(false, 2.0, 1.0, 40.0, 0.0);
        a[genome::G_HUE] = 0.0;
        let mut b = genome::initial(false, 0.5, 1.0, 40.0, 0.0);
        b[genome::G_HUE] = 0.5;
        w.spawn(100.0, 100.0, CARNIVORE, 20.0, 0, &a);
        w.spawn(103.0, 100.0, CARNIVORE, 0.5, 0, &b);
        w.cover = vec![0.0; 2];
        w.build_grid();
        // 大きいほうが勝つまで繰り返す（負けた小さいほうは手持ちが少なく死ぬ）
        for _ in 0..50 {
            if !w.alive[1] {
                break;
            }
            w.cooldown[0] = 0;
            w.fight(0, 1, death::FOUGHT);
        }
        assert!(!w.alive[1], "小さいほうが死ぬ");
        let c = w.plants.cell_index(103.0, 100.0);
        assert!(w.carrion[c] > 0.0, "体が死体になる");
        assert!(w.census.deaths[CARNIVORE as usize][death::FOUGHT as usize] >= 1);
    }

    #[test]
    fn 仲間が見えない時間を数える() {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 9.0, 0, &g);
        for _ in 0..5 {
            w.step();
        }
        assert_eq!(w.alone[0], 5, "一人なら毎 tick 増える");
        // 目の前に仲間を置くと 0 に戻る
        let (x, y, hx, hy) = (w.xs[0], w.ys[0], w.hx[0], w.hy[0]);
        w.spawn(x + hx * 10.0, y + hy * 10.0, HERBIVORE, 9.0, 0, &g);
        w.step();
        let i = w.index_of(1).unwrap();
        assert_eq!(w.alone[i], 0, "仲間が見えたら 0");
    }

    #[test]
    fn 一度に複数産むと子は小さく_休みは渡した量に比例する() {
        let born_seed = |litter: f32, invest: f32, seed: u64| {
            let mut p = small_params();
            p.herb_initial = 0.0;
            p.carn_initial = 0.0;
            p.sunlight = 0.0;
            let mut w = World::new(seed, p);
            let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
            g[genome::G_LITTER] = litter;
            g[genome::G_INVEST] = invest;
            w.spawn(100.0, 100.0, HERBIVORE, 20.0, 0, &g);
            w.spawn(103.0, 100.0, HERBIVORE, 5.0, 0, &g);
            w.live(0, 0.0, true, Mate::Partner(1));
            let rest = w.rest[0];
            w.apply_births();
            let kids: Vec<usize> = (2..w.count()).collect();
            (kids.len(), kids.iter().map(|&k| w.growth[k]).fold(0.0f32, f32::max), rest)
        };
        let born = |litter: f32, invest: f32| born_seed(litter, invest, 1);
        let (one, one_size, one_rest) = born(1.0, 0.5);
        let (three, three_size, three_rest) = born(3.0, 0.5);
        // 端数は確率: 1.5 なら 1 体か 2 体
        let twins = (0..40).filter(|&seed| born_seed(1.5, 0.5, seed).0 == 2).count();
        assert!((8..=32).contains(&twins), "1.5 なら半分くらいが双子: {twins}/40");
        assert_eq!((one, three), (1, 3));
        assert!(three_size < one_size * 0.5, "3 体なら 1 体あたり小さい: {one_size} → {three_size}");
        assert_eq!(one_rest, three_rest, "渡した合計が同じなら休みも同じ");
        let (_, _, small_rest) = born(1.0, 0.1);
        assert!(small_rest < one_rest / 3, "少し渡すだけなら休みは短い: {one_rest} → {small_rest}");
    }

    #[test]
    fn 系統ごとに直近の死因を数える() {
        let mut w = empty_world();
        w.plants.fill(0.0);
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        g[genome::G_HUE] = 0.1;
        w.spawn(100.0, 100.0, HERBIVORE, 0.001, 0, &g);
        g[genome::G_HUE] = 0.6;
        w.spawn(300.0, 100.0, HERBIVORE, 0.001, 0, &g);
        w.step(); // どちらも餓死する
        let s = w.lineage_stats(HERBIVORE, 0.05, 0.1);
        assert_eq!(s[17 + death::STARVED as usize], 1.0, "範囲内の 1 体だけ");
        assert_eq!(w.lineage_stats(HERBIVORE, 0.0, 1.0)[17 + death::STARVED as usize], 2.0);
        // 期間を過ぎると消える
        for _ in 0..DEATH_WINDOW {
            w.step();
        }
        assert_eq!(w.lineage_stats(HERBIVORE, 0.0, 1.0)[17 + death::STARVED as usize], 0.0);
    }

    #[test]
    fn 小さい個体ほど草に隠れやすい() {
        // 仲間の見え方で比べる（獲物は好みの大きさの範囲で絞られるので使わない）
        let seen = |size: f32| {
            let mut w = empty_world();
            let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
            let g = genome::initial(true, size, 1.0, 30.0, 0.0);
            w.spawn(115.0, 100.0, HERBIVORE, 5.0, 0, &g);
            face(&mut w, 0, 1.0, 0.0);
            w.params.cover_strength = 0.6;
            w.cover = vec![0.0, 1.0];
            w.build_grid();
            w.sense(0).kin.is_some()
        };
        // 15px 先。大きさ 1 なら 30 × (1 − 0.6) = 12px、大きさ 2 なら 30 × (1 − 0.42) = 17px まで見える
        assert!(!seen(1.0), "大きさ 1 は草むらに隠れる");
        assert!(seen(2.0), "大きい個体は草むらでも見つかる");
    }

    #[test]
    fn 小さい体ほど早く大人になり寿命も短い() {
        let mut w = empty_world();
        let small = genome::initial(true, 0.5, 1.0, 30.0, 0.0);
        let big = genome::initial(true, 2.0, 1.0, 30.0, 0.0);
        let (ts, tb) = (genome::traits(&small, w.params.max_turn), genome::traits(&big, w.params.max_turn));
        w.spawn_grown(100.0, 100.0, HERBIVORE, 50.0, 1, &small, ts, 0.5);
        w.spawn_grown(200.0, 100.0, HERBIVORE, 50.0, 1, &big, tb, 0.5);
        for _ in 0..10 {
            w.live(0, 0.0, false, Mate::Missing);
            w.live(1, 0.0, false, Mate::Missing);
        }
        assert!(w.growth[0] > w.growth[1], "小さいほうが速く育つ");
        assert!(w.lifespan(0) < w.lifespan(1) * 1.6, "寿命の比は大きさの比の 0.25 乗程度");
    }

    #[test]
    fn 仲間を攻撃する個体は届く距離の同じ系統を攻撃する() {
        let mut w = empty_world();
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        genome::set_kin_attack_bias(&mut g, 5.0);
        g[genome::G_HUE] = 0.3;
        w.spawn(100.0, 100.0, HERBIVORE, 9.0, 0, &g);
        let mut calm = g.clone();
        genome::set_kin_attack_bias(&mut calm, -5.0);
        w.spawn(103.0, 100.0, HERBIVORE, 9.0, 0, &calm); // 同じ系統（同じ系統色）
        face(&mut w, 0, 1.0, 0.0);
        face(&mut w, 1, -1.0, 0.0);
        w.step();
        assert!(w.census.kin_fights[HERBIVORE as usize] >= 1, "仲間を攻撃した");
        assert_eq!(w.census.fights[HERBIVORE as usize], 0, "敵ではないので、敵への攻撃には数えない");
    }

    #[test]
    fn 周りの草の量を届く層の量と質で数える() {
        let mut w = empty_world();
        w.params.graze_floor = 0.0;
        w.params.graze_floor_min = 0.0;
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, 1.0, 1.0, 30.0, 0.0));
        w.plants.fill(0.0);
        assert_eq!(w.plants_around(0), 0.0);
        let cap = w.params.plant_capacity;
        w.plants.fill(cap);
        // 大きさ 1 は地面の草（50%）と茂み（30%、質 0.85）に届き、枝には届かない
        let p = &w.params;
        let expect = 9.0 * ((1.0 - p.plant_share_shrub - p.plant_share_tree) + p.plant_share_shrub * p.shrub_quality);
        assert!((w.plants_around(0) - expect).abs() < 1e-4, "{} / {expect}", w.plants_around(0));
    }

    #[test]
    fn 高い層ほど大きい体でないと届かない() {
        let mut w = empty_world();
        for (k, size) in [0.3f32, 1.0, 3.0].iter().enumerate() {
            w.spawn(100.0 * (k + 1) as f32, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, *size, 1.0, 30.0, 0.0));
        }
        let near = |a: [f32; LAYERS], b: [f32; LAYERS]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4);
        assert!(near(w.reach(0), [1.0, 0.0, 0.0]), "小さい体は地面の草だけ");
        assert!(near(w.reach(1), [1.0, 1.0, 0.0]), "大きさ 1 は茂みまで");
        assert!(near(w.reach(2), [1.0, 1.0, 1.0]), "大きい体は枝まで");
    }

    #[test]
    fn 届く層のうち量と質の大きい層から食べ届かない層は減らない() {
        let mut w = empty_world();
        w.params.graze_floor = 0.0;
        w.params.graze_floor_min = 0.0;
        w.params.herb_metabolism = 0.0;
        w.spawn(100.0, 100.0, HERBIVORE, 1.0, 0, &genome::initial(true, 0.3, 1.0, 30.0, 0.0));
        w.spawn(200.0, 100.0, HERBIVORE, 1.0, 0, &genome::initial(true, 3.0, 1.0, 30.0, 0.0));
        let (small, big) = (w.plants.cell_index(100.0, 100.0), w.plants.cell_index(200.0, 100.0));
        // 地面の草はわずか、枝はたっぷり
        for c in [small, big] {
            w.plants.layer[0][c] = 0.01;
            w.plants.layer[1][c] = 0.0;
            w.plants.layer[2][c] = 5.0;
        }
        w.plants.sum_layers();
        assert!(w.graze(0));
        assert!(w.graze(1));
        assert_eq!(w.plants.layer[2][small], 5.0, "小さい体は枝に届かない");
        assert!(w.plants.layer[0][small] < 0.01, "小さい体は地面の草を食べる");
        assert!(w.plants.layer[2][big] < 5.0, "大きい体は量の多い枝を食べる");
        assert_eq!(w.plants.layer[0][big], 0.01);
        let sum: f32 = (0..LAYERS).map(|k| w.plants.layer[k][big]).sum();
        assert!((w.plants.biomass[big] - sum).abs() < 1e-6, "合計は層の和のまま");
    }

    #[test]
    fn 遠くの景色で食べ物の多い場所の方向がわかり大きい体ほど遠くまで見える() {
        let mut w = empty_world();
        w.params.graze_floor_min = 0.0;
        w.params.graze_floor = 0.0;
        for size in [1.0f32, 0.3] {
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, size, 1.0, 30.0, 0.0));
        }
        // 生まれた植生をその場所に合わせる（植生の合い具合で値が落ちないように）
        for i in 0..2 {
            w.natal[i] = w.plants.vegetation[w.plants.cell_index(400.0, 100.0)];
        }
        w.plants.fill(0.0);
        // x = 384〜511（区画 3 列目）の、y = 0〜127 の区画だけに草を置く
        let cs = w.plants.cell_size as usize;
        let blk = crate::plants::BLOCK * cs;
        for r in 0..crate::plants::BLOCK {
            for c in 3 * crate::plants::BLOCK..4 * crate::plants::BLOCK {
                let i = r * w.plants.cols + c;
                if w.plants.fertility[i] > 0.0 {
                    w.plants.set_cell(i, w.params.plant_capacity);
                }
            }
        }
        assert_eq!(blk, 128);
        w.plants.update_landscape();
        let dir = |w: &World, i: usize| w.far_food_direction(i, &w.reach(i), w.graze_floor(i));
        assert_eq!(dir(&w, 0), Some((1.0, 0.0)), "大きさ 1 は 320px 先の草が見える");
        assert_eq!(dir(&w, 1), None, "大きさ 0.3 は見渡せる距離が約 214px で届かない");
    }

    #[test]
    fn 大きさの分布の段階は対数で等間隔に分ける() {
        let (lo, hi) = genome::SIZE_RANGE;
        assert_eq!(size_hist_bin(lo), 0);
        assert_eq!(size_hist_bin(0.01), 0, "範囲より小さいものは最初の段階");
        assert_eq!(size_hist_bin(hi), SIZE_HIST_BINS - 1, "上限は最後の段階");
        // 2 倍ごとに同じ段階の数だけ進む
        let step = |s: f32| size_hist_bin(s * 2.0) as i32 - size_hist_bin(s) as i32;
        assert!((step(0.2) - step(1.0)).abs() <= 1);
        let mut w = empty_world();
        for size in [0.3f32, 0.3, 2.0] {
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &genome::initial(true, size, 1.0, 30.0, 0.0));
        }
        let h = w.adult_size_histogram(HERBIVORE);
        assert_eq!(h[size_hist_bin(0.3)], 2);
        assert_eq!(h[size_hist_bin(2.0)], 1);
        assert_eq!(h.iter().sum::<u32>(), 3);
        assert_eq!(w.adult_size_histogram(CARNIVORE).iter().sum::<u32>(), 0);
    }

    #[test]
    fn 大きい口ほど地面の草を高く食べ残す() {
        let mut w = empty_world();
        w.params.herb_metabolism = 0.0;
        w.spawn(100.0, 100.0, HERBIVORE, 1.0, 0, &genome::initial(true, 0.3, 1.0, 30.0, 0.0));
        w.spawn(200.0, 100.0, HERBIVORE, 1.0, 0, &genome::initial(true, 3.0, 1.0, 30.0, 0.0));
        assert!(w.graze_floor(0) < w.graze_floor(1), "大きい体ほど食べ残す");
        // どんなに小さくても、下限より低くは食べられない
        assert!((w.graze_floor(0) - w.params.graze_floor_min).abs() < 1e-6, "大きさ 0.3 は下限に当たる: {}", w.graze_floor(0));
        let (small, big) = (w.plants.cell_index(100.0, 100.0), w.plants.cell_index(200.0, 100.0));
        w.plants.fill(0.0);
        // 地面の草だけが、両者の食べ残す高さの中間まである
        for c in [small, big] {
            let cap = w.params.plant_capacity * w.plants.ground_cap(c);
            w.plants.set_cell(c, 0.0);
            w.plants.layer[0][c] = cap * (w.graze_floor(0) + w.graze_floor(1)) / 2.0;
        }
        w.plants.sum_layers();
        let before = w.plants.layer[0][big];
        assert!(w.plants_around(0) > 0.0, "小さい体には短い草が見える");
        assert_eq!(w.plants_around(1), 0.0, "大きい体には食べられる草がない");
        assert!(w.graze(0), "小さい体は短い草を食べる");
        assert!(!w.graze(1), "大きい体は短い草を噛み取れない");
        assert_eq!(w.plants.layer[0][big], before);
    }

    #[test]
    fn 目と脳の費用は小さい体ほど安い() {
        let cost = |size: f32| {
            let mut w = empty_world();
            w.params.move_cost = 0.0;
            w.params.herb_metabolism = 0.0;
            w.params.speed_upkeep = 0.0;
            w.params.thermo_cost = 0.0;
            w.params.signal_cost = 0.0;
            let g = genome::initial(true, size, 1.0, 30.0, 0.0);
            w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
            w.live(0, 0.0, false, Mate::Missing);
            5.0 - w.energy[0]
        };
        let (small, unit) = (cost(0.1), cost(1.0));
        assert!((small / unit - 0.1f32.powf(0.75)).abs() < 1e-3, "大きさの 0.75 乗に比例: {small} / {unit}");
    }

    #[test]
    fn 判断を並列に計算しても順番に計算しても同じ歴史になる() {
        let mut p = small_params();
        p.herb_initial = 1500.0;
        p.carn_initial = 120.0;
        let (mut a, mut b) = (World::new(5, p.clone()), World::new(5, p));
        a.use_parallel = true; // parallel 機能がなければ順番に計算する（そのときは同じ処理どうしの比較になる）
        b.use_parallel = false;
        for _ in 0..300 {
            a.step();
            b.step();
        }
        assert_eq!(a.id, b.id);
        assert_eq!(a.xs, b.xs);
        assert_eq!(a.energy, b.energy);
        assert_eq!(a.genomes, b.genomes);
    }

    #[test]
    fn 近くの個体を選べる() {
        let w = World::new(2, small_params());
        let id = w.id_near(w.xs[10], w.ys[10], 0.5);
        assert_eq!(id, w.id[10]);
        assert_eq!(w.id_near(-1000.0, -1000.0, 0.5), 0);
        // 種類を指定すると、より近くに別の種類がいても指定した種類を選ぶ
        let i = (0..w.count()).find(|&i| w.kind[i] == CARNIVORE).unwrap();
        let id = w.id_near_kind(w.xs[i], w.ys[i], 1000.0, Some(HERBIVORE));
        assert_ne!(id, 0);
        assert_eq!(w.kind[w.index_of(id).unwrap()], HERBIVORE);
    }

    /// 茂みと枝のある平らなマスに草食を 1 体置いた世界
    fn nest_world() -> World {
        let mut w = empty_world();
        let g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        w.spawn(100.0, 100.0, HERBIVORE, 5.0, 0, &g);
        let c = w.plants.cell_index(100.0, 100.0);
        w.plants.layer[1][c] = 2.0;
        w.plants.layer[2][c] = 1.0;
        w.plants.sum_layers();
        w
    }

    #[test]
    fn 巣を作ると茂みと枝を材料にして出来が上がり帳尻は合う() {
        let mut w = nest_world();
        let c = w.plants.cell_index(100.0, 100.0);
        let before = w.total_energy();
        let expected_before = w.ledger.expected_total();
        assert!(w.build_nest(0));
        let p = &w.params;
        let take = p.nest_material * w.traits[0].size;
        assert!((w.nest[c] - take / p.nest_material_need).abs() < 1e-6, "出来の良さ {}", w.nest[c]);
        assert!((w.plants.layer[1][c] + w.plants.layer[2][c] - (3.0 - take)).abs() < 1e-5, "材料の分だけ減る");
        assert!((w.plants.layer[1][c] / w.plants.layer[2][c] - 2.0).abs() < 1e-4, "茂みと枝から、ある量に比例して取る");
        assert_eq!((w.nest_owner[c], w.home[0]), (w.id[0], c as u32));
        // 帳簿: 材料とエネルギーの分だけ減り、実際の量と合う
        let spent = before - w.total_energy();
        let booked = expected_before - w.ledger.expected_total();
        assert!((spent - booked).abs() < 1e-5, "実際 {spent} 帳簿 {booked}");

        // 出来上がったら作らない。材料がなければ作れない
        w.nest[c] = 1.0;
        assert!(!w.build_nest(0));
        w.nest[c] = 0.0;
        w.plants.layer[1][c] = 0.0;
        w.plants.layer[2][c] = 0.0;
        w.plants.sum_layers();
        assert!(!w.build_nest(0), "草原の草だけでは作れない");
    }

    #[test]
    fn 巣は崩れて消え覚えていた個体は忘れる() {
        let mut w = nest_world();
        let c = w.plants.cell_index(100.0, 100.0);
        assert!(w.build_nest(0));
        let q = w.nest[c];
        w.decay_nests();
        assert!((w.nest[c] - q * (1.0 - w.params.nest_decay)).abs() < 1e-7);
        w.nest[c] = NEST_MIN * 1.0001;
        w.decay_nests();
        assert_eq!((w.nest[c], w.nest_owner[c]), (0.0, 0));
        w.live(0, 0.0, false, Mate::Missing);
        assert_eq!(w.home[0], NO_HOME, "崩れた巣は忘れる");
    }

    #[test]
    fn 巣の中では体温の費用が下がり見つかりにくい() {
        let cost_and_conceal = |nest: f32| {
            let mut w = nest_world();
            let c = w.plants.cell_index(100.0, 100.0);
            w.nest[c] = nest;
            w.traits[0].temp_pref = w.terrain.temperature_at(100.0, 100.0) - 10.0;
            w.cover = vec![0.0];
            w.build_grid();
            let conceal = w.conceal[0];
            let e = w.energy[0];
            w.live(0, 0.0, false, Mate::Missing);
            (e - w.energy[0], conceal)
        };
        let (cost_out, conceal_out) = cost_and_conceal(0.0);
        let (cost_in, conceal_in) = cost_and_conceal(1.0);
        let w = nest_world();
        let thermal = w.params.thermo_cost * 10.0 * w.traits[0].thermal_scale;
        let saved = cost_out - cost_in;
        assert!((saved - w.params.nest_insulation * thermal).abs() < 1e-5, "体温の費用が減る: {cost_out} → {cost_in}");
        assert!(conceal_in > conceal_out + 0.3, "巣の中は見つかりにくい: {conceal_out} → {conceal_in}");
    }

    #[test]
    fn 巣の場所と出来を入力に渡し子は親の巣を覚えて生まれる() {
        let mut w = nest_world();
        face(&mut w, 0, 1.0, 0.0);
        w.cover = vec![0.0];
        w.build_grid();
        assert_eq!(w.plan(0).input[genome::IN_NEST_HERE], 0.0);
        assert!(w.build_nest(0));
        let input = w.plan(0).input;
        assert!(input[genome::IN_NEST_HERE] > 0.0);
        assert_eq!((input[genome::IN_NEST_FWD], input[genome::IN_NEST_RIGHT]), (0.0, 0.0), "巣のマスにいれば方向は 0");
        assert_eq!(input[genome::IN_NEST_AWAY], 0.0);
        // 離れると、巣（後ろ）の方向を指す
        w.xs[0] = 300.0;
        let input = w.plan(0).input;
        assert!(input[genome::IN_NEST_FWD] < -0.99);
        assert_eq!(input[genome::IN_NEST_AWAY], 1.0, "巣から離れている");
        // 子は親の巣を覚えて生まれる
        w.energy[0] = 20.0;
        w.reproduce(0, 0.0, None);
        w.apply_births();
        assert!(w.count() >= 2);
        assert_eq!(w.home[1], w.home[0]);
    }

    #[test]
    fn 巣作りの初期配線があれば繁殖できる状態で巣にいれば巣を作る() {
        let mut g = genome::initial(true, 1.0, 1.0, 30.0, 0.0);
        let mut input = [0.0; genome::N_IN];
        input[genome::IN_READY] = 1.0;
        assert!(genome::think(&g, &input)[genome::OUT_BUILD] < 0.0, "初期の脳は巣を作らない");
        genome::add_nest_wiring(&mut g, 2.0);
        assert!(genome::think(&g, &input)[genome::OUT_BUILD] > 0.0, "繁殖できる状態なら作る");
        input[genome::IN_NEST_AWAY] = 1.0;
        assert!(genome::think(&g, &input)[genome::OUT_BUILD] < 0.0, "巣から離れていれば作らない");
        input[genome::IN_NEST_AWAY] = 0.0;
        input[genome::IN_READY] = 0.0;
        assert!(genome::think(&g, &input)[genome::OUT_BUILD] < 0.0, "繁殖できない状態なら作らない");
    }

    #[test]
    fn 肉食から遠ざかっていた獲物ほど仕留めにくい() {
        // 肉食は (100, 100)、獲物はその右 (104, 100)。前の tick の獲物の動きだけを変えて比べる
        let chance = |vel: (f32, f32)| {
            let mut w = hunting_world(0.5);
            w.build_grid();
            w.prev_vel = vec![(0.0, 0.0), vel];
            w.hunt_chance(0, 1)
        };
        let still = chance((0.0, 0.0));
        let max = hunting_world(0.5).traits[0].max_speed;
        assert_eq!(still, chance((-1.0, 0.0)), "肉食に向かってくる獲物には効かない");
        assert_eq!(still, chance((0.0, 1.0)), "横に動いている獲物には効かない");
        let half = chance((0.5 * max, 0.0));
        let p = hunting_world(0.5).params.escape_strength;
        assert!((half - still * (1.0 - 0.5 * p)).abs() < 1e-6, "肉食の半分の速さで遠ざかる: {still} → {half}");
        assert!((chance((2.0 * max, 0.0)) - still * (1.0 - p)).abs() < 1e-6, "肉食より速ければ下げ幅は上限");
    }

    #[test]
    fn 動いた量を覚え次のtickの始めに写す() {
        let mut w = hunting_world(0.5);
        w.hx[1] = 1.0;
        w.hy[1] = 0.0;
        w.move_agent(1, 1.0);
        assert!((w.vel[1].0 - 1.0).abs() < 1e-6 && w.vel[1].1.abs() < 1e-6);
        w.move_agent(1, 0.0);
        assert_eq!(w.vel[1], (0.0, 0.0), "止まれば 0");
    }
}

//! JS との境界。ここにある `extern "C"` 関数が WASM の export になる。
//!
//! 境界を越えるのは数値とメモリ上のアドレスだけ。
//! 文字列（パラメータ名）は、JS が scratch_ptr() の指す領域に UTF-8 で書き込んでから渡す。
//! JS は render_buffer_ptr() などが返すアドレスから、WASM のメモリを TypedArray として直接読む。

pub mod genome;
mod grid;
pub mod params;
pub mod plants;
pub mod rng;
pub mod terrain;
pub mod world;

use params::Params;
use std::cell::RefCell;
use world::{CARNIVORE, HERBIVORE, LINEAGE_STATS, RENDER_STRIDE, World};

/// inspect() が書き込む配列の要素数。各要素の意味は inspect() のコメントを参照
const INSPECT_LEN: usize = 44;
/// summary() が書き込む配列の要素数。各要素の意味は summary() のコメントを参照
const SUMMARY_LEN: usize = 29;

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
    /// 次の init() で使うパラメータ。init() 前に設定できるよう、世界とは別に持つ
    static PARAMS: RefCell<Params> = RefCell::new(Params::default());
    /// JS から文字列を受け取るための作業領域
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// 選択中の個体の情報。id が f32 の精度を超えうるので f64 で持つ
    static INSPECT: RefCell<[f64; INSPECT_LEN]> = const { RefCell::new([0.0; INSPECT_LEN]) };
    static SUMMARY: RefCell<[f64; SUMMARY_LEN]> = const { RefCell::new([0.0; SUMMARY_LEN]) };
    /// 系統の統計（選んだ系統と種類全体の 2 組）
    static LINEAGE: RefCell<[f64; LINEAGE_STATS * 2]> = const { RefCell::new([0.0; LINEAGE_STATS * 2]) };
    /// 地形を JS に渡すための作業領域
    static TERRAIN: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|w| f(w.borrow_mut().as_mut().expect("init() が呼ばれていない")))
}

/// len バイト書き込める作業領域のアドレスを返す
#[unsafe(no_mangle)]
pub extern "C" fn scratch_ptr(len: u32) -> *mut u8 {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        s.resize(len as usize, 0);
        s.as_mut_ptr()
    })
}

/// 作業領域の先頭 name_len バイトを名前として、パラメータを設定する。
/// 知らない名前なら 0 を返す。世界があれば即座に反映する（世界の大きさなどはリセットまで効かない）
#[unsafe(no_mangle)]
pub extern "C" fn set_param(name_len: u32, value: f32) -> u32 {
    let name = SCRATCH.with(|s| String::from_utf8_lossy(&s.borrow()[..name_len as usize]).into_owned());
    let ok = PARAMS.with(|p| p.borrow_mut().set(&name, value));
    if ok {
        WORLD.with(|w| {
            if let Some(world) = w.borrow_mut().as_mut() {
                world.params.set(&name, value);
            }
        });
    }
    ok as u32
}

/// 現在のパラメータで世界を作り直す
#[unsafe(no_mangle)]
pub extern "C" fn init(seed: u32) {
    let params = PARAMS.with(|p| p.borrow().clone());
    let world = World::new(seed as u64, params);
    WORLD.with(|w| *w.borrow_mut() = Some(world));
}

/// ticks 回だけ進める
#[unsafe(no_mangle)]
pub extern "C" fn step(ticks: u32) {
    with_world(|w| {
        for _ in 0..ticks {
            w.step();
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn agent_count() -> u32 {
    with_world(|w| w.count() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn current_tick() -> f64 {
    with_world(|w| w.tick as f64)
}

/// 描画用バッファを最新にして、その先頭アドレスを返す。
/// 中身は [x, y, 種類, 大きさ, 標識色, 向き] × agent_count() の f32 配列。
/// 次に step() か init() を呼ぶと中身もアドレスも変わりうるので、すぐにコピーすること。
#[unsafe(no_mangle)]
pub extern "C" fn render_buffer_ptr() -> *const f32 {
    with_world(|w| {
        w.fill_render();
        w.render.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn render_stride() -> u32 {
    RENDER_STRIDE as u32
}

/// (x, y) から radius 以内で最も近い個体を選択し、その id を返す。いなければ 0 を返し、選択は解除される
#[unsafe(no_mangle)]
pub extern "C" fn select_at(x: f32, y: f32, radius: f32) -> f64 {
    with_world(|w| {
        let id = w.id_near(x, y, radius);
        w.select(id);
        id as f64
    })
}

/// (x, y) から radius 以内で最も近い、種類 kind の個体を選択し、その id を返す。
/// 同じ種類がいなければ種類を問わず最寄りを選ぶ。それもいなければ 0 を返し、選択は解除される
#[unsafe(no_mangle)]
pub extern "C" fn select_near_kind(x: f32, y: f32, radius: f32, kind: u32) -> f64 {
    with_world(|w| {
        let mut id = w.id_near_kind(x, y, radius, Some(kind as u8));
        if id == 0 {
            id = w.id_near(x, y, radius);
        }
        w.select(id);
        id as f64
    })
}

/// 選択を解除する
#[unsafe(no_mangle)]
pub extern "C" fn deselect() {
    with_world(|w| w.select(0))
}

/// 選択中の個体の情報を書き込み、その先頭アドレスを返す（f64 × INSPECT_LEN）。
///  0: 状態（0 = 選択なし、1 = 生存、2 = 死亡）
///  1: id            2: 種類            3: x              4: y
///  5: エネルギー     6: 繁殖に必要なエネルギー
///  7: 年齢           8: 寿命            9: 行動（world::state）
/// 10: 直前の速度    11: 直前の消費エネルギー
/// 12: 子の数        13: 世代
/// 14: 死因（world::death）  15: 死んだ tick
/// 16: 視野の半径    17: 大きさ        18: 最高速度       19: 子への投資    20: 標識色（0〜1）
/// 21: 直前に曲がった角度（ラジアン、正で右）  22: 直前の速さ（最高速度に対する割合）
/// 23: 繁殖の出力（正なら繁殖したい）          24: 食べるの出力（正なら食べたい）
/// 25: 隠れ具合（足元の草で、相手の視野が何割縮むか。繁殖シグナルで目立つ分を引く）
/// 26: 蓄えられるエネルギーの上限
/// 27: 繁殖相手が近くにいるか（1 = いる。無性生殖の設定なら常に 1）
/// 29: 繁殖相手との相性（繁殖が成功する確率。相手がいなければ負）
/// 30〜34: 欲求ごとの優先度（0〜1）[食べる, 繁殖, 逃げる, 仲間, 自由]
/// 35: 警戒の度合い（0〜1、草食のみ）
/// 36: 生まれた場所の植生（肥沃度）  37: 今いる場所の草の食べやすさ（0〜1、植生の違いによる）
/// 38: 今いる場所の平均気温  39: 好みの気温
/// 40: 繁殖シグナルの強さ（0〜1）  41: 体の調子（飢えと老いで最高速度に掛かる倍率）
/// 42: 視野角（ラジアン）  43: 次に繁殖できるまでの残り tick 数
/// 28: 成長度（1 で成熟）。17 の大きさは遺伝子が決める大人の大きさで、いまの大きさは 17 × 28
/// 死亡時は 0, 1, 14, 15 だけが有効で、他は前回の値のまま残る
#[unsafe(no_mangle)]
pub extern "C" fn inspect() -> *const f64 {
    INSPECT.with(|buf| {
        let mut out = buf.borrow_mut();
        with_world(|w| {
            let watch = &w.watch;
            if watch.id == 0 {
                out[0] = 0.0;
                return;
            }
            out[1] = watch.id as f64;
            if let Some((cause, tick)) = watch.death {
                out[0] = 2.0;
                out[14] = cause as f64;
                out[15] = tick as f64;
                return;
            }
            let Some(i) = w.index_of(watch.id) else {
                out[0] = 0.0;
                return;
            };
            let p = &w.params;
            out[0] = 1.0;
            out[2] = w.kind[i] as f64;
            out[3] = w.xs[i] as f64;
            out[4] = w.ys[i] as f64;
            out[5] = w.energy[i] as f64;
            let t = w.traits[i];
            out[6] = w.repro_threshold(i) as f64;
            out[7] = w.age[i] as f64;
            out[8] = w.lifespan(i) as f64;
            out[9] = w.state[i] as f64;
            out[10] = watch.speed as f64;
            out[11] = watch.cost as f64;
            out[12] = w.children[i] as f64;
            out[13] = w.generation[i] as f64;
            out[16] = t.vision as f64;
            out[17] = t.adult_size as f64;
            out[18] = t.max_speed as f64;
            out[19] = t.invest as f64;
            out[20] = t.hue as f64;
            out[21] = watch.turn as f64;
            out[22] = watch.throttle as f64;
            out[23] = watch.output[genome::OUT_REPRODUCE] as f64;
            out[24] = watch.output[genome::OUT_EAT] as f64;
            out[25] = (p.cover_strength * w.cover[i] * (1.0 - p.signal_exposure * w.signal[i])) as f64;
            out[26] = w.energy_cap(i) as f64;
            out[27] = if watch.mate_nearby { 1.0 } else { 0.0 };
            out[28] = w.growth[i] as f64;
            out[29] = watch.mate_compat as f64;
            for k in 0..genome::N_DRIVES {
                out[30 + k] = watch.priorities[k] as f64;
            }
            out[35] = w.vigilance[i] as f64;
            out[36] = w.natal[i] as f64;
            out[37] = w.habitat_fit(i, w.terrain.soil_at(w.xs[i], w.ys[i])) as f64;
            out[38] = w.terrain.temperature_at(w.xs[i], w.ys[i]) as f64;
            out[39] = t.temp_pref as f64;
            out[40] = watch.signal as f64;
            out[41] = watch.vigor as f64;
            out[42] = t.fov as f64;
            out[43] = w.rest[i] as f64;
        });
        out.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn inspect_len() -> u32 {
    INSPECT_LEN as u32
}

/// 世界全体の集計を書き込み、その先頭アドレスを返す（f64 × SUMMARY_LEN）。
///  0: 植物量の合計
///  1〜6: 草食の [個体数, 平均の大きさ, 平均の最高速度, 平均の視野, 平均の子への投資, 平均の世代]
///  7〜12: 肉食の同じもの
/// 13〜17: 草食の群れの度合い（World::herd_stats の 5 つ）
/// 18〜22: 肉食の同じもの
/// 23, 24: これまでの交配の回数と、相性で失敗した回数
/// 25: 季節（−1〜1、日射が最も多い時期に 1）  26: 1 年のうちの位置（0〜1、0.25 で日射が最も多い）
/// 27, 28: 草食・肉食の繁殖シグナルの平均
#[unsafe(no_mangle)]
pub extern "C" fn summary() -> *const f64 {
    SUMMARY.with(|buf| {
        let mut out = buf.borrow_mut();
        with_world(|w| {
            out[0] = w.plants.total();
            out[1..7].copy_from_slice(&w.trait_means(HERBIVORE));
            out[7..13].copy_from_slice(&w.trait_means(CARNIVORE));
            out[13..18].copy_from_slice(&w.herd_stats(HERBIVORE));
            out[18..23].copy_from_slice(&w.herd_stats(CARNIVORE));
            out[23] = w.matings as f64;
            out[24] = w.failed_matings as f64;
            out[25] = w.season() as f64;
            out[26] = w.season_phase() as f64;
            out[27] = w.mean_signal(HERBIVORE);
            out[28] = w.mean_signal(CARNIVORE);
        });
        out.as_ptr()
    })
}

/// 系統の統計を書き込み、その先頭アドレスを返す（f64 × lineage_len()）。
/// 前半が選んだ系統（種類 kind のうち、標識色が hue_from から幅 hue_width の個体）、後半が種類全体。並びは world::LINEAGE_STATS
#[unsafe(no_mangle)]
pub extern "C" fn lineage_stats(kind: u32, hue_from: f32, hue_width: f32) -> *const f64 {
    LINEAGE.with(|buf| {
        let mut out = buf.borrow_mut();
        with_world(|w| {
            out[..LINEAGE_STATS].copy_from_slice(&w.lineage_stats(kind as u8, hue_from, hue_width));
            out[LINEAGE_STATS..].copy_from_slice(&w.lineage_stats(kind as u8, 0.0, 1.0));
        });
        out.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lineage_len() -> u32 {
    (LINEAGE_STATS * 2) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn summary_len() -> u32 {
    SUMMARY_LEN as u32
}

/// 地形の配列（plant_cols() × plant_rows() の f32、行優先）を書き込み、その先頭アドレスを返す。
/// 水のマスは −1、陸のマスは肥沃度（0〜1）。地形は世界を作り直すまで変わらないので、作ったときに 1 度読めばよい
#[unsafe(no_mangle)]
pub extern "C" fn terrain_ptr() -> *const f32 {
    TERRAIN.with(|buf| {
        let mut out = buf.borrow_mut();
        with_world(|w| {
            out.clear();
            out.extend(w.terrain.water.iter().zip(&w.terrain.soil).map(|(&water, &f)| if water { -1.0 } else { f }));
        });
        out.as_ptr()
    })
}

/// 平均気温の配列（plant_cols() × plant_rows() の f32、行優先、度）の先頭アドレス。世界を作り直すまで変わらない
#[unsafe(no_mangle)]
pub extern "C" fn temperature_ptr() -> *const f32 {
    with_world(|w| w.terrain.temperature.as_ptr())
}

/// 死体の量の配列（plant_cols() × plant_rows() の f32、行優先）の先頭アドレス
#[unsafe(no_mangle)]
pub extern "C" fn carrion_ptr() -> *const f32 {
    with_world(|w| w.carrion.as_ptr())
}

/// 植物量の配列（plant_cols() × plant_rows() の f32、行優先）の先頭アドレス
#[unsafe(no_mangle)]
pub extern "C" fn plant_ptr() -> *const f32 {
    with_world(|w| w.plants.biomass.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn plant_cols() -> u32 {
    with_world(|w| w.plants.cols as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn plant_rows() -> u32 {
    with_world(|w| w.plants.rows as u32)
}

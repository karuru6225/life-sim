//! JS との境界。ここにある `extern "C"` 関数が WASM の export になる。
//!
//! 境界を越えるのは数値とメモリ上のアドレスだけ。
//! 文字列（パラメータ名）は、JS が scratch_ptr() の指す領域に UTF-8 で書き込んでから渡す。
//! JS は render_buffer_ptr() などが返すアドレスから、WASM のメモリを TypedArray として直接読む。

mod grid;
pub mod params;
pub mod plants;
pub mod rng;
pub mod world;

use params::Params;
use std::cell::RefCell;
use world::{CARNIVORE, HERBIVORE, RENDER_STRIDE, World};

/// inspect() が書き込む配列の要素数。各要素の意味は inspect() のコメントを参照
const INSPECT_LEN: usize = 17;

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
    /// 次の init() で使うパラメータ。init() 前に設定できるよう、世界とは別に持つ
    static PARAMS: RefCell<Params> = RefCell::new(Params::default());
    /// JS から文字列を受け取るための作業領域
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// 選択中の個体の情報。id が f32 の精度を超えうるので f64 で持つ
    static INSPECT: RefCell<[f64; INSPECT_LEN]> = const { RefCell::new([0.0; INSPECT_LEN]) };
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
pub extern "C" fn herbivore_count() -> u32 {
    with_world(|w| w.count_kind(HERBIVORE) as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn carnivore_count() -> u32 {
    with_world(|w| w.count_kind(CARNIVORE) as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn plant_total() -> f64 {
    with_world(|w| w.plants.total())
}

#[unsafe(no_mangle)]
pub extern "C" fn current_tick() -> f64 {
    with_world(|w| w.tick as f64)
}

/// 描画用バッファを最新にして、その先頭アドレスを返す。
/// 中身は [x, y, 種類] × agent_count() の f32 配列。
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
/// 16: 視野の半径
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
            let herb = w.kind[i] == HERBIVORE;
            out[0] = 1.0;
            out[2] = w.kind[i] as f64;
            out[3] = w.xs[i] as f64;
            out[4] = w.ys[i] as f64;
            out[5] = w.energy[i] as f64;
            out[6] = if herb { p.herb_repro_threshold } else { p.carn_repro_threshold } as f64;
            out[7] = w.age[i] as f64;
            out[8] = if herb { p.herb_max_age } else { p.carn_max_age } as f64;
            out[9] = w.state[i] as f64;
            out[10] = watch.speed as f64;
            out[11] = watch.cost as f64;
            out[12] = w.children[i] as f64;
            out[13] = w.generation[i] as f64;
            out[16] = if herb { p.herb_vision } else { p.carn_vision } as f64;
        });
        out.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn inspect_len() -> u32 {
    INSPECT_LEN as u32
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

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

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
    /// 次の init() で使うパラメータ。init() 前に設定できるよう、世界とは別に持つ
    static PARAMS: RefCell<Params> = RefCell::new(Params::default());
    /// JS から文字列を受け取るための作業領域
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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

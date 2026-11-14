//! JS との境界。ここにある `extern "C"` 関数が WASM の export になる。
//!
//! 境界を越えるのは数値とメモリ上のアドレスだけ。
//! JS は render_buffer_ptr() が返すアドレスから、WASM のメモリを Float32Array として直接読む。

mod grid;
pub mod rng;
pub mod world;

use std::cell::RefCell;
use world::{RENDER_STRIDE, World};

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
}

fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|w| f(w.borrow_mut().as_mut().expect("init() が呼ばれていない")))
}

/// 世界を作り直す
#[unsafe(no_mangle)]
pub extern "C" fn init(seed: u32, count: u32, width: f32, height: f32) {
    let world = World::new(seed as u64, count as usize, width, height);
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
/// 中身は [x, y, 近傍数] × agent_count() の f32 配列。
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

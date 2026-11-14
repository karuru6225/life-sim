// sim.wasm を読み込み、固定タイムステップで回し続ける。
// 描画用データはバッファにコピーしてメインスレッドへ転送（transfer）する。

import wasmUrl from './wasm/sim.wasm?url';
import { TICK_HZ } from './config';
import type { FromWorker, ToWorker } from './protocol';

interface SimExports {
  memory: WebAssembly.Memory;
  init(seed: number, count: number, width: number, height: number): void;
  step(ticks: number): void;
  agent_count(): number;
  current_tick(): number;
  render_buffer_ptr(): number;
  render_stride(): number;
}

/** 転送用バッファの最大数。メインスレッドが返してこない限り、これ以上は作らない */
const MAX_BUFFERS = 3;
/** 描画に送る最短間隔。画面の更新より速く送っても無駄になる */
const MIN_PUBLISH_INTERVAL_MS = 1000 / 60 - 1;
/** 最大速モードで、メッセージ処理に戻るまでに計算し続ける時間 */
const MAX_MODE_SLICE_MS = 25;
/** 倍速で処理が追いつかないとき、遅れを何秒分まで持ち越すか */
const MAX_BACKLOG_SECONDS = 0.25;
const STATS_INTERVAL_MS = 500;

const simReady: Promise<SimExports> = fetch(wasmUrl)
  .then((res) => res.arrayBuffer())
  .then((bytes) => WebAssembly.instantiate(bytes, {}))
  .then(({ instance }) => instance.exports as unknown as SimExports);

let sim: SimExports | null = null;
let generation = 0;
let speed = 1;
let paused = false;

let stride = 3;
let bufferBytes = 0;
let pool: ArrayBuffer[] = [];
let allocated = 0;
let dirty = false;
let lastPublish = 0;

let accumulator = 0;
let lastTime = performance.now();

let statStart = performance.now();
let statTicks = 0;
let statStepMs = 0;

function post(msg: FromWorker, transfer: Transferable[] = []): void {
  self.postMessage(msg, { transfer });
}

function handle(s: SimExports, msg: ToWorker): void {
  switch (msg.type) {
    case 'init': {
      s.init(msg.seed, msg.count, msg.width, msg.height);
      sim = s;
      generation++;
      stride = s.render_stride();
      bufferBytes = s.agent_count() * stride * 4;
      pool = [];
      allocated = 0;
      accumulator = 0;
      dirty = true;
      post({ type: 'ready', generation, count: s.agent_count(), stride });
      break;
    }
    case 'setSpeed':
      speed = msg.speed;
      accumulator = 0;
      break;
    case 'setPaused':
      paused = msg.paused;
      break;
    case 'returnBuffer':
      // 個体数が変わる前のバッファはサイズが合わないので捨てる
      if (msg.buffer.byteLength === bufferBytes && pool.length < MAX_BUFFERS) {
        pool.push(msg.buffer);
      }
      break;
  }
}

self.onmessage = async (e: MessageEvent<ToWorker>) => {
  handle(await simReady, e.data);
};

function stepTimed(s: SimExports, ticks: number): void {
  if (ticks <= 0) return;
  const t0 = performance.now();
  s.step(ticks);
  statStepMs += performance.now() - t0;
  statTicks += ticks;
  dirty = true;
}

function advance(s: SimExports, now: number): void {
  if (paused) return;
  if (speed === Infinity) {
    const end = now + MAX_MODE_SLICE_MS;
    do stepTimed(s, 1);
    while (performance.now() < end);
    return;
  }
  const tps = TICK_HZ * speed;
  accumulator += ((now - lastTime) * tps) / 1000;
  const ticks = Math.floor(accumulator);
  accumulator -= ticks;
  stepTimed(s, ticks);
  // 計算が追いつかない分は持ち越さずに捨てる（溜めると操作への反応が遅れ続ける）
  if (accumulator > tps * MAX_BACKLOG_SECONDS) accumulator = 0;
}

function publish(s: SimExports, now: number): void {
  if (!dirty || now - lastPublish < MIN_PUBLISH_INTERVAL_MS) return;
  let buffer = pool.pop();
  if (!buffer) {
    if (allocated >= MAX_BUFFERS) return;
    buffer = new ArrayBuffer(bufferBytes);
    allocated++;
  }
  const count = s.agent_count();
  const ptr = s.render_buffer_ptr();
  // memory.buffer は WASM のメモリが伸びると作り直されるので、毎回取り直す
  new Float32Array(buffer).set(new Float32Array(s.memory.buffer, ptr, count * stride));
  post({ type: 'frame', generation, buffer, count, tick: s.current_tick() }, [buffer]);
  dirty = false;
  lastPublish = now;
}

function reportStats(now: number): void {
  const elapsed = now - statStart;
  if (elapsed < STATS_INTERVAL_MS) return;
  post({
    type: 'stats',
    ticksPerSecond: (statTicks * 1000) / elapsed,
    msPerTick: statTicks > 0 ? statStepMs / statTicks : 0,
  });
  statStart = now;
  statTicks = 0;
  statStepMs = 0;
}

function loop(): void {
  const now = performance.now();
  if (sim) {
    advance(sim, now);
    publish(sim, performance.now());
    reportStats(now);
  }
  lastTime = now;
  setTimeout(loop, 0);
}

loop();

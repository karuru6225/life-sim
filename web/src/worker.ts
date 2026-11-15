// sim.wasm を読み込み、固定タイムステップで回し続ける。
// 描画用データはバッファにコピーしてメインスレッドへ転送（transfer）する。

import wasmUrl from './wasm/sim.wasm?url';
import { AUTO_SUNLIGHT, TICK_HZ } from './config';
import type { FromWorker, LineageSelection, ToWorker } from './protocol';

interface SimExports {
  memory: WebAssembly.Memory;
  scratch_ptr(len: number): number;
  set_param(nameLen: number, value: number): number;
  init(seed: number): void;
  step(ticks: number): void;
  agent_count(): number;
  summary(): number;
  summary_len(): number;
  current_tick(): number;
  render_buffer_ptr(): number;
  render_stride(): number;
  plant_ptr(): number;
  carrion_ptr(): number;
  lineage_stats(kind: number, from: number, width: number): number;
  lineage_len(): number;
  plant_cols(): number;
  plant_rows(): number;
  terrain_ptr(): number;
  temperature_ptr(): number;
  select_at(x: number, y: number, radius: number): number;
  select_near_kind(x: number, y: number, radius: number, kind: number): number;
  deselect(): void;
  inspect(): number;
  inspect_len(): number;
}

/** 転送用バッファの最大数。メインスレッドが返してこない限り、これ以上は作らない */
const MAX_BUFFERS = 3;
/** 個体数が増えるたびに作り直さないよう、少し大きめに確保する */
const BUFFER_HEADROOM = 1.5;
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

const encoder = new TextEncoder();

let sim: SimExports | null = null;
let generation = 0;
let speed = 1;
let paused = false;
/** 個体を選択中か（選択中ならフレームごとに個体の情報を添える） */
let watching = false;

// 日射量の自動調整（config.ts の AUTO_SUNLIGHT を参照）
let autoSunlight = true;
/** スライダーで設定された日射量 */
let baseSunlight = 0;
/** 設定値に掛ける倍率。計算が重いと 1 から下がる */
let sunlightFactor = 1;
/** 計算時間の移動平均（ms/tick）。0 はまだ測っていない */
let smoothedMs = 0;

let pool: ArrayBuffer[] = [];
let allocated = 0;
let dirty = false;
let lastPublish = 0;

let accumulator = 0;
let lastTime = performance.now();

/** 前回の集計時点での、交配の累計回数と失敗の累計回数 */
/** 統計を取る系統（null なら取らない） */
let lineageSel: LineageSelection | null = null;
let lastMatings = 0;
let lastFailed = 0;

let statStart = performance.now();
let statTicks = 0;
let statStepMs = 0;

function post(msg: FromWorker, transfer: Transferable[] = []): void {
  self.postMessage(msg, { transfer });
}

/** 日射量だけは自動調整の倍率を掛けてから渡す */
function setSunlight(s: SimExports): void {
  setParam(s, 'sunlight', baseSunlight * sunlightFactor);
}

/** パラメータを設定する。日射量は設定値として受け取り、自動調整の倍率を掛けて渡す */
function applyParam(s: SimExports, name: string, value: number): void {
  if (name === 'sunlight') {
    baseSunlight = value;
    setSunlight(s);
  } else {
    setParam(s, name, value);
  }
}

function setParam(s: SimExports, name: string, value: number): void {
  const bytes = encoder.encode(name);
  const ptr = s.scratch_ptr(bytes.length);
  new Uint8Array(s.memory.buffer, ptr, bytes.length).set(bytes);
  if (!s.set_param(bytes.length, value)) {
    console.warn(`シミュレーションが知らないパラメータ: ${name}（sim/src/params.rs と config.ts の名前を確認）`);
  }
}

function handle(s: SimExports, msg: ToWorker): void {
  switch (msg.type) {
    case 'init':
      sunlightFactor = 1;
      smoothedMs = 0;
      lastMatings = 0;
      lastFailed = 0;
      for (const [name, value] of Object.entries(msg.params)) applyParam(s, name, value);
      s.init(msg.seed);
      sim = s;
      generation++;
      watching = false;
      pool = [];
      allocated = 0;
      accumulator = 0;
      dirty = true;
      {
        const cols = s.plant_cols();
        const rows = s.plant_rows();
        const terrain = new Float32Array(cols * rows);
        terrain.set(new Float32Array(s.memory.buffer, s.terrain_ptr(), cols * rows));
        const temperature = new Float32Array(cols * rows);
        temperature.set(new Float32Array(s.memory.buffer, s.temperature_ptr(), cols * rows));
        post(
          {
            type: 'ready',
            generation,
            terrain: terrain.buffer,
            temperature: temperature.buffer,
            terrainCols: cols,
            terrainRows: rows,
          },
          [terrain.buffer, temperature.buffer],
        );
      }
      break;
    case 'setParam':
      applyParam(s, msg.name, msg.value);
      break;
    case 'setSpeed':
      speed = msg.speed;
      accumulator = 0;
      break;
    case 'setPaused':
      paused = msg.paused;
      break;
    case 'setAutoSunlight':
      autoSunlight = msg.enabled;
      if (!autoSunlight) {
        sunlightFactor = 1;
        setSunlight(s);
      }
      break;
    case 'select': {
      if (!sim) break;
      const id =
        msg.kind === undefined
          ? s.select_at(msg.x, msg.y, msg.radius)
          : s.select_near_kind(msg.x, msg.y, msg.radius, msg.kind);
      watching = id !== 0;
      dirty = true; // 一時停止中でも選択結果をすぐ描けるようにする
      post({ type: 'selected', id });
      break;
    }
    case 'deselect':
      if (!sim) break;
      s.deselect();
      watching = false;
      dirty = true;
      break;
    case 'setLineage':
      lineageSel = msg.lineage;
      break;
    case 'returnBuffer':
      if (pool.length < MAX_BUFFERS) pool.push(msg.buffer);
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
    buffer = new ArrayBuffer(0);
    allocated++;
  }

  const agentCount = s.agent_count();
  const stride = s.render_stride();
  const plantCols = s.plant_cols();
  const plantRows = s.plant_rows();
  const agentFloats = agentCount * stride;
  const plantFloats = plantCols * plantRows;
  const bytes = (agentFloats + plantFloats * 2) * 4;
  if (buffer.byteLength < bytes) {
    buffer = new ArrayBuffer(Math.ceil((bytes * BUFFER_HEADROOM) / 4) * 4);
  }

  const out = new Float32Array(buffer);
  // memory.buffer は WASM のメモリが伸びると作り直されるので、ポインタを取ってから毎回取り直す
  const agentsPtr = s.render_buffer_ptr();
  out.set(new Float32Array(s.memory.buffer, agentsPtr, agentFloats), 0);
  const plantsPtr = s.plant_ptr();
  out.set(new Float32Array(s.memory.buffer, plantsPtr, plantFloats), agentFloats);
  out.set(new Float32Array(s.memory.buffer, s.carrion_ptr(), plantFloats), agentFloats + plantFloats);

  const inspect = watching ? Array.from(new Float64Array(s.memory.buffer, s.inspect(), s.inspect_len())) : null;

  post(
    { type: 'frame', generation, buffer, agentCount, stride, plantCols, plantRows, tick: s.current_tick(), inspect },
    [buffer],
  );
  dirty = false;
  lastPublish = now;
}

/** 計算時間に合わせて日射量の倍率を少しずつ動かす */
function adjustSunlight(s: SimExports, msPerTick: number, ticks: number): void {
  smoothedMs = smoothedMs === 0 ? msPerTick : smoothedMs + AUTO_SUNLIGHT.smoothing * (msPerTick - smoothedMs);
  if (!autoSunlight) return;
  const before = sunlightFactor;
  const step = Math.min(AUTO_SUNLIGHT.maxStep, AUTO_SUNLIGHT.stepPerTick * ticks);
  if (smoothedMs > AUTO_SUNLIGHT.targetMs) {
    sunlightFactor = Math.max(AUTO_SUNLIGHT.minFactor, sunlightFactor * (1 - step));
  } else if (smoothedMs < AUTO_SUNLIGHT.targetMs * AUTO_SUNLIGHT.recoverBelow) {
    sunlightFactor = Math.min(1, sunlightFactor * (1 + step));
  }
  if (sunlightFactor !== before) setSunlight(s);
}

function reportStats(s: SimExports, now: number): void {
  const elapsed = now - statStart;
  if (elapsed < STATS_INTERVAL_MS) return;
  // 一時停止中は計っていないので、調整もしない
  if (statTicks > 0) adjustSunlight(s, statStepMs / statTicks, statTicks);
  // 並びは sim/src/lib.rs の summary() を参照
  const v = new Float64Array(s.memory.buffer, s.summary(), s.summary_len());
  const means = (o: number, herd: number, signal: number) => ({
    count: v[o],
    size: v[o + 1],
    speed: v[o + 2],
    vision: v[o + 3],
    invest: v[o + 4],
    generation: v[o + 5],
    clustering: v[herd],
    alignment: v[herd + 1],
    alignSame: v[herd + 2],
    alignOther: v[herd + 3],
    sameShare: v[herd + 4],
    signal: v[signal],
  });
  // 交配の回数は累計なので、前回からの差で失敗率を出す
  const matings = v[23] - lastMatings;
  const failed = v[24] - lastFailed;
  lastMatings = v[23];
  lastFailed = v[24];
  post({
    type: 'stats',
    ticksPerSecond: (statTicks * 1000) / elapsed,
    msPerTick: statTicks > 0 ? statStepMs / statTicks : 0,
    plantTotal: v[0],
    sunlight: baseSunlight * sunlightFactor,
    sunlightFactor,
    matingFailRate: matings > 0 ? failed / matings : NaN,
    season: v[25],
    seasonPhase: v[26],
    herbivore: means(1, 13, 27),
    carnivore: means(7, 18, 28),
    lineage: lineageSel
      ? Array.from(
          new Float64Array(
            s.memory.buffer,
            s.lineage_stats(lineageSel.kind, lineageSel.from, lineageSel.width),
            s.lineage_len(),
          ),
        )
      : null,
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
    reportStats(sim, now);
  }
  lastTime = now;
  setTimeout(loop, 0);
}

loop();

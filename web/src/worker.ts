// sim.wasm を読み込み、固定タイムステップで回し続ける。
// 描画用データはバッファにコピーしてメインスレッドへ転送（transfer）する。

import wasmUrl from './wasm/sim.wasm?url';
import { AUTO_SUNLIGHT, TICK_HZ } from './config';
import { SAVE_MAGIC, type FromWorker, type LineageSelection, type SaveHeader, type ToWorker } from './protocol';
import { SIM_PARAMS } from './config';

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
  plant_layer_ptr(k: number): number;
  wood_ptr(): number;
  elevation_ptr(): number;
  carrion_ptr(): number;
  select_id(id: number): number;
  save_world(): number;
  save_len(): number;
  save_format_version(): number;
  load_buffer(len: number): number;
  load_world(): number;
  load_error_ptr(): number;
  load_error_len(): number;
  get_param(nameLen: number): number;
  temperature_now_ptr(): number;
  fertility_now_ptr(): number;
  select_random_in_lineage(kind: number, from: number, width: number, nonce: number): number;
  brain_ptr(): number;
  brain_inputs(): number;
  brain_outputs(): number;
  brain_hidden(): number;
  brain_h1(): number;
  brain_net_ptr(): number;
  brain_net_len(): number;
  energy_history_ptr(): number;
  energy_history_len(): number;
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
  history_count(): number;
  history_version(): number;
  history_last_tick(): number;
  history_record_len(): number;
  history_size_bins(i: number): number;
  history_export(from: number): number;
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
/** 脳の重みを最後に送った個体の id（選んだ個体が変わったら送り直す） */
let netSentId = -1;

// 日射量の自動調整（config.ts の AUTO_SUNLIGHT を参照）
let autoSunlight = AUTO_SUNLIGHT.enabledByDefault;
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
/** 地図に描く植物の層（-1 は合計）。メインスレッドが setPlantLayer で選ぶ */
let plantLayer = -1;
/** 世界を作り直した直後は、累計の集計の基準を取り直す（読み込んだ世界は累計が 0 から始まらないため） */
let rebaseline = true;
let lastMatings = 0;
let lastHunts = 0;
let lastKills = 0;
let lastFailed = 0;

/** 長期の統計の記録を最後に送ったときの、記録の数・並べ直しの回数・標本の tick */
let historyCount = 0;
let historyVersion = -1;
let historyLastTick = -1;

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

/** 世界を作り直した（初期化・読み込み）あとの共通処理。集計を区切り直し、地形を送る */
function onNewWorld(s: SimExports): void {
  sunlightFactor = 1;
  smoothedMs = 0;
  rebaseline = true;
  sim = s;
  generation++;
  watching = false;
  netSentId = -1;
  pool = [];
  allocated = 0;
  accumulator = 0;
  dirty = true;
  const cols = s.plant_cols();
  const rows = s.plant_rows();
  const terrain = new Float32Array(cols * rows);
  terrain.set(new Float32Array(s.memory.buffer, s.terrain_ptr(), cols * rows));
  const temperature = new Float32Array(cols * rows);
  temperature.set(new Float32Array(s.memory.buffer, s.temperature_ptr(), cols * rows));
  // wood_ptr は terrain_ptr と同じ置き場を使うので、地形を写し取った後に読む
  const wood = new Float32Array(cols * rows);
  wood.set(new Float32Array(s.memory.buffer, s.wood_ptr(), cols * rows));
  const elevation = new Float32Array(cols * rows);
  elevation.set(new Float32Array(s.memory.buffer, s.elevation_ptr(), cols * rows));
  post(
    {
      type: 'ready',
      generation,
      terrain: terrain.buffer,
      temperature: temperature.buffer,
      wood: wood.buffer,
      elevation: elevation.buffer,
      terrainCols: cols,
      terrainRows: rows,
    },
    [terrain.buffer, temperature.buffer, wood.buffer, elevation.buffer],
  );
  // 季節で変わる地図のデータもすぐ送る（地図の表示を切り替えたときに空にならないように）
  sendClimate(s);
  lastClimate = performance.now();
  // 長期の統計の記録の全部（読み込んだ世界なら保存までの記録も）。ready の後に送る（メインは ready で記録を消すため）
  syncHistory(s, true);
}

const decoder = new TextDecoder();

/** gzip で圧縮する（ブラウザに組み込みの CompressionStream を使う） */
async function gzip(parts: BlobPart[]): Promise<ArrayBuffer> {
  return new Response(new Blob(parts).stream().pipeThrough(new CompressionStream('gzip'))).arrayBuffer();
}

async function gunzip(data: ArrayBuffer): Promise<ArrayBuffer> {
  return new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer();
}

/** 世界を保存する: 見出し（JSON）＋世界のバイト列を gzip で圧縮して返す */
async function saveWorld(s: SimExports): Promise<void> {
  const ptr = s.save_world();
  const payload = new Uint8Array(s.memory.buffer, ptr, s.save_len()).slice();
  const v = new Float64Array(s.memory.buffer, s.summary(), s.summary_len());
  const header: SaveHeader = {
    format: s.save_format_version(),
    commit: __COMMIT__,
    savedAt: new Date().toISOString(),
    tick: s.current_tick(),
    herbivores: v[1],
    carnivores: v[7],
  };
  const headerBytes = encoder.encode(JSON.stringify(header));
  const len = new Uint8Array(4);
  new DataView(len.buffer).setUint32(0, headerBytes.length, true);
  const buffer = await gzip([encoder.encode(SAVE_MAGIC), len, headerBytes, payload]);
  const filename = `life-sim-tick${header.tick}-${header.savedAt.slice(0, 19).replace(/[-:T]/g, '')}.lifesim.gz`;
  post({ type: 'saved', buffer, filename, bytes: payload.length }, [buffer]);
}

/** 保存したファイルから世界を読み込む */
async function loadWorld(s: SimExports, file: ArrayBuffer): Promise<void> {
  const fail = (message: string) => post({ type: 'loaded', ok: false, message, params: null, tick: 0 });
  let data: Uint8Array;
  const raw = new Uint8Array(file);
  if (raw[0] === 0x1f && raw[1] === 0x8b) {
    // gzip（ブラウザで保存したファイル）
    try {
      data = new Uint8Array(await gunzip(file));
    } catch {
      return fail('ファイルを展開できない（壊れている）');
    }
  } else {
    // 圧縮していないファイル（画面なしの実行器で書き出したもの）
    data = raw;
  }
  const magic = encoder.encode(SAVE_MAGIC);
  if (data.length < magic.length + 4 || !magic.every((b, k) => data[k] === b)) return fail('life-sim の保存ファイルではない');
  const headerLen = new DataView(data.buffer, data.byteOffset + magic.length, 4).getUint32(0, true);
  const headerStart = magic.length + 4;
  let header: SaveHeader;
  try {
    header = JSON.parse(decoder.decode(data.subarray(headerStart, headerStart + headerLen)));
  } catch {
    return fail('ファイルの見出しが壊れている');
  }
  const payload = data.subarray(headerStart + headerLen);
  const ptr = s.load_buffer(payload.length);
  new Uint8Array(s.memory.buffer, ptr, payload.length).set(payload);
  const ok = s.load_world() === 1;
  const note = decoder.decode(new Uint8Array(s.memory.buffer, s.load_error_ptr(), s.load_error_len()).slice());
  if (!ok) return fail(`読み込めない: ${note}（保存したコード ${header.commit}、今のコード ${__COMMIT__}）`);
  onNewWorld(s);
  // 画面のパラメータ（日射量など）を、読み込んだ世界の値にそろえる
  const params: Record<string, number> = {};
  for (const name of Object.keys(SIM_PARAMS)) {
    const bytes = encoder.encode(name);
    new Uint8Array(s.memory.buffer, s.scratch_ptr(bytes.length), bytes.length).set(bytes);
    const value = s.get_param(bytes.length);
    if (!Number.isNaN(value)) params[name] = value;
  }
  if (params.sunlight !== undefined) baseSunlight = params.sunlight;
  const notes = [];
  if (header.commit !== __COMMIT__) notes.push(`別の版で保存されたデータ（保存したコード ${header.commit}、今のコード ${__COMMIT__}）`);
  if (note) notes.push(note);
  post({ type: 'loaded', ok: true, message: notes.join('。'), params, tick: header.tick });
}

function handle(s: SimExports, msg: ToWorker): void {
  switch (msg.type) {
    case 'init':
      for (const [name, value] of Object.entries(msg.params)) applyParam(s, name, value);
      s.init(msg.seed);
      onNewWorld(s);
      break;
    case 'save':
      if (sim) void saveWorld(s);
      break;
    case 'load':
      void loadWorld(s, msg.buffer);
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
    case 'selectRandomInLineage': {
      if (!sim) break;
      const l = msg.lineage;
      const id = s.select_random_in_lineage(l.kind, l.from, l.width, Math.floor(Math.random() * 2 ** 31));
      watching = id !== 0;
      dirty = true;
      post({ type: 'selected', id });
      break;
    }
    case 'selectId': {
      if (!sim) break;
      const id = s.select_id(msg.id);
      watching = id !== 0;
      dirty = true;
      post({ type: 'selected', id });
      break;
    }
    case 'deselect':
      if (!sim) break;
      s.deselect();
      watching = false;
      netSentId = -1;
      dirty = true;
      break;
    case 'setLineage':
      lineageSel = msg.lineage;
      break;
    case 'setPlantLayer':
      plantLayer = msg.layer;
      dirty = true;
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
  syncHistory(s);
}

/**
 * 長期の統計の記録（Rust が持つ）に新しい標本が入っていたら、メインスレッドへ送る。
 * 並べ直し（隣どうしをまとめた）があったときや all のときは全部、そうでなければ最後に送った記録（まとめ途中だったもの）以降
 */
function syncHistory(s: SimExports, all = false): void {
  const lastTick = s.history_last_tick();
  const version = s.history_version();
  if (!all && lastTick === historyLastTick && version === historyVersion) return;
  const count = s.history_count();
  const reset = all || version !== historyVersion || count < historyCount;
  const from = reset ? 0 : Math.max(0, historyCount - 1);
  const recordLen = s.history_record_len();
  const ptr = s.history_export(from);
  const data = new Float64Array(s.memory.buffer, ptr, (count - from) * recordLen).slice();
  post(
    {
      type: 'history',
      generation,
      reset,
      from,
      recordLen,
      bins: s.history_size_bins(0),
      sizeRange: [s.history_size_bins(1), s.history_size_bins(2)],
      data: data.buffer,
    },
    [data.buffer],
  );
  historyCount = count;
  historyVersion = version;
  historyLastTick = lastTick;
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
  const plantsPtr = plantLayer < 0 ? s.plant_ptr() : s.plant_layer_ptr(plantLayer);
  out.set(new Float32Array(s.memory.buffer, plantsPtr, plantFloats), agentFloats);
  out.set(new Float32Array(s.memory.buffer, s.carrion_ptr(), plantFloats), agentFloats + plantFloats);

  const inspect = watching ? Array.from(new Float64Array(s.memory.buffer, s.inspect(), s.inspect_len())) : null;
  let brain: { input: number[]; output: number[]; hidden: number[]; h1: number } | null = null;
  let energyHistory: number[] | null = null;
  let brainNet: number[] | undefined;
  let brainNetId: number | undefined;
  if (watching) {
    const nIn = s.brain_inputs();
    const nOut = s.brain_outputs();
    const io = Array.from(new Float32Array(s.memory.buffer, s.brain_ptr(), nIn + nOut + s.brain_hidden()));
    brain = { input: io.slice(0, nIn), output: io.slice(nIn, nIn + nOut), hidden: io.slice(nIn + nOut), h1: s.brain_h1() };
    // 重みは生涯変わらないので、選んだ個体が変わったときだけ送る
    const id = inspect ? inspect[1] : 0;
    if (id !== netSentId) {
      const ptr = s.brain_net_ptr();
      brainNet = Array.from(new Float32Array(s.memory.buffer, ptr, s.brain_net_len()));
      brainNetId = id;
      netSentId = id;
    }
    energyHistory = Array.from(new Float32Array(s.memory.buffer, s.energy_history_ptr(), s.energy_history_len()));
  }

  post(
    { type: 'frame', generation, buffer, agentCount, stride, plantCols, plantRows, tick: s.current_tick(), inspect, brain, energyHistory, brainNet, brainNetId },
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

/** 季節で変わる地図のデータ（今の気温と育ちやすさ）を送る */
let lastClimate = 0;
const CLIMATE_INTERVAL_MS = 1000;
function sendClimate(s: SimExports): void {
  const n = s.plant_cols() * s.plant_rows();
  const temperature = new Float32Array(n);
  temperature.set(new Float32Array(s.memory.buffer, s.temperature_now_ptr(), n));
  const fertility = new Float32Array(n);
  fertility.set(new Float32Array(s.memory.buffer, s.fertility_now_ptr(), n));
  post({ type: 'climate', generation, temperature: temperature.buffer, fertility: fertility.buffer }, [
    temperature.buffer,
    fertility.buffer,
  ]);
}

function reportStats(s: SimExports, now: number): void {
  if (now - lastClimate >= CLIMATE_INTERVAL_MS) {
    sendClimate(s);
    lastClimate = now;
  }
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
  if (rebaseline) {
    [lastMatings, lastFailed, lastHunts, lastKills] = [v[23], v[24], v[29], v[30]];
    rebaseline = false;
  }
  // 交配の回数は累計なので、前回からの差で失敗率を出す
  const matings = v[23] - lastMatings;
  const failed = v[24] - lastFailed;
  lastMatings = v[23];
  lastFailed = v[24];
  // 狩りの回数も累計なので、前回からの差で成功率を出す
  const hunts = v[29] - lastHunts;
  const kills = v[30] - lastKills;
  lastHunts = v[29];
  lastKills = v[30];
  post({
    type: 'stats',
    ticksPerSecond: (statTicks * 1000) / elapsed,
    msPerTick: statTicks > 0 ? statStepMs / statTicks : 0,
    plantTotal: v[0],
    sunlight: baseSunlight * sunlightFactor,
    sunlightFactor,
    matingFailRate: matings > 0 ? failed / matings : NaN,
    huntSuccessRate: hunts > 0 ? kills / hunts : NaN,
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

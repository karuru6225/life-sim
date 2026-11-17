import { Application, Container, Graphics } from 'pixi.js';
import {
  AUTO_RESELECT,
  BODY_RADIUS,
  COLORS,
  DIMMED_ALPHA,
  HIGHLIGHT_MIN_RADIUS_PX,
  MIN_BODY_RADIUS_PX,
  MIN_SELECT_SCALE,
  SEED,
  SELECT_RADIUS_PX,
  SIM_PARAMS,
  WORLD_SIZES,
  type SimParams,
  type WorldSize,
} from './config';
import { CARNIVORE, HERBIVORE, INSPECT, LINEAGE, type FromWorker, type LineageSelection, type ToWorker } from './protocol';
import { AgentLayer, type ColorMode } from './render/agents';
import { Camera } from './render/camera';
import { PlantLayer } from './render/plants';
import { TerrainLayer, type MapMode } from './render/terrain';
import { SelectionMarker } from './render/selection';
import { Hud, type Highlight, type HighlightStyle, type Layers } from './ui/hud';
import { Inspector, Toast } from './ui/inspector';
import { LineageHistogram, LineagePanel } from './ui/lineage';
import { StatsWindow } from './ui/stats';
import { BrainWindow } from './ui/brain';
import './style.css';

type Frame = Extract<FromWorker, { type: 'frame' }>;

const params: SimParams = { ...SIM_PARAMS };

/** 世界の広さ。ブラウザに覚えておき、次に開いたときも同じ広さで作る */
const WORLD_SIZE_KEY = 'life-sim.worldSize';
let worldSize: WorldSize = 'medium';
try {
  const saved = localStorage.getItem(WORLD_SIZE_KEY);
  if (saved && saved in WORLD_SIZES) worldSize = saved as WorldSize;
} catch {
  // 覚えておけない環境では中
}
/** 広さの選択肢をパラメータに反映する */
function applyWorldSizeParams(size: WorldSize): void {
  const def = WORLD_SIZES[size];
  params.world_width = def.width;
  params.world_height = def.height;
  params.herb_initial = def.herbivores;
  params.carn_initial = def.carnivores;
}
applyWorldSizeParams(worldSize);

const app = new Application();
await app.init({
  resizeTo: window,
  background: COLORS.background,
  antialias: false,
  preference: 'webgl',
  resolution: window.devicePixelRatio,
  autoDensity: true,
});
document.getElementById('stage')!.append(app.canvas);

const world = new Container();
app.stage.addChild(world);

const terrain = new TerrainLayer(params.plant_cell);
const plants = new PlantLayer(params.plant_cell, params.plant_capacity);
// 死体は 8 で最も濃く、0.2 以上あれば 25% の濃さで見えるようにする（それ未満は描かない）
const carrion = new PlantLayer(params.plant_cell, 8, COLORS.carrion, 0.25, 0.2);
world.addChild(terrain.sprite, plants.sprite, carrion.sprite);

// 世界の外枠。トーラスなので端はつながっているが、どこまでが世界かは見せておく
const border = new Graphics();
world.addChild(border);

// 拡大しても角張らないよう、テクスチャは高解像度で作る（見た目の大きさは radius のまま）
const dot = (radius: number) =>
  app.renderer.generateTexture({
    target: new Graphics().circle(0, 0, radius).fill(0xffffff),
    resolution: 8,
    antialias: true,
  });
/** 三角形の頂点（外接円の半径 r、+x を指す） */
const arrowPoints = (r: number) => [r, 0, -0.7 * r, 0.75 * r, -0.7 * r, -0.75 * r];

/**
 * 進行方向（+x）を指す、少し細長い三角形。肉食を形で見分けるため（系統色の表示でも色に頼らず区別できる）。
 * 同じ半径の円より小さく見えるので、外接円の半径を 1.3 倍にしている
 */
const arrow = (radius: number) => {
  const r = radius * 1.3;
  return app.renderer.generateTexture({
    target: new Graphics().poly(arrowPoints(r)).fill(0xffffff),
    resolution: 8,
    antialias: true,
  });
};
// 大きさは個体ごとの倍率で表す
const herbivores = new AgentLayer(dot(BODY_RADIUS.herbivore), COLORS.herbivore, 1, false, params.world_width, params.world_height);
// 肉食は数が少なく埋もれやすいので、一回り大きい三角形で上に重ね、進行方向へ向ける
const carnivores = new AgentLayer(
  arrow(BODY_RADIUS.herbivore),
  COLORS.carnivore,
  BODY_RADIUS.carnivore / BODY_RADIUS.herbivore,
  true,
  params.world_width,
  params.world_height,
);

// 「枠線」の強調に使う輪。体より一回り大きい輪を、個体の上に重ねて描く。
// テクスチャの大きさで体との比を決めているので、描画倍率は体と同じでよい
const ringTexture = (shape: 'circle' | 'arrow', radius: number) =>
  app.renderer.generateTexture({
    target:
      shape === 'circle'
        ? new Graphics().circle(0, 0, radius * 1.7).stroke({ width: radius * 0.4, color: 0xffffff })
        : new Graphics().poly(arrowPoints(radius * 1.3 * 1.6)).stroke({ width: radius * 0.4, color: 0xffffff }),
    resolution: 8,
    antialias: true,
  });
const herbOutline = new AgentLayer(
  ringTexture('circle', BODY_RADIUS.herbivore),
  COLORS.marker,
  1,
  false,
  params.world_width,
  params.world_height,
);
const carnOutline = new AgentLayer(
  ringTexture('arrow', BODY_RADIUS.herbivore),
  COLORS.marker,
  BODY_RADIUS.carnivore / BODY_RADIUS.herbivore,
  true,
  params.world_width,
  params.world_height,
);
const marker = new SelectionMarker();
/** 選んだ系統の重心（十字）と広がり（円） */
const lineageMark = new Graphics();
/** 選択の印の入れ物。印そのものは選択に合わせて表示を切り替えるので、表示するものの切り替えはこちらで行う */
const markerLayer = new Container();
markerLayer.addChild(marker.graphics);
world.addChild(
  herbivores.container,
  carnivores.container,
  herbOutline.container,
  carnOutline.container,
  lineageMark,
  markerLayer,
);

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker, transfer: Transferable[] = []) => worker.postMessage(msg, transfer);

let generation = -1;
let latest: Frame | null = null;
let speed = 1;
let paused = false;
let colorMode: ColorMode = 'kind';
/** 地図に描く植物の層（-1 は合計、0: 地面の草、1: 茂み、2: 枝） */
let plantLayer = -1;

/**
 * 層 k が 1 マスに溜まりうる最大の量（その層だけを描くとき、この量で色が最も濃い）。
 * 林の度合いで層の割合が変わるので、最も林（茂み・枝）か最も草原（地面の草）の場所での上限。sim/src/plants.rs の share_at と同じ式
 */
function plantLayerMax(k: number): number {
  const p = params;
  if (k < 0) return p.plant_capacity;
  const c = Math.min(1, Math.max(0, p.woodland_contrast));
  const upper = p.plant_share_shrub + p.plant_share_tree;
  const share = [1 - upper * (1 - c), p.plant_share_shrub * (1 + c), p.plant_share_tree * (1 + c)][k];
  return p.plant_capacity * Math.min(1, Math.max(0.05, share));
}
let highlight: Highlight = 'none';
let highlightStyle: HighlightStyle = 'dim';
/** 統計を見ている系統（null なら選んでいない）。選んでいる間は、系統の外の個体と、もう一方の種類を薄く描く */
let lineageSel: LineageSelection | null = null;

/** 地図の上に重ねるものの表示。ブラウザに覚えておき、次に開いたときも同じにする */
const LAYERS_KEY = 'life-sim.layers';
const layers: Layers = {
  terrain: true,
  plants: true,
  carrion: true,
  herbivores: true,
  carnivores: true,
  border: true,
  lineage: true,
  marker: true,
};
try {
  Object.assign(layers, JSON.parse(localStorage.getItem(LAYERS_KEY) ?? '{}'));
} catch {
  // 覚えておけない環境（プライベートウィンドウなど）では既定のまま
}

/** 表示するものを反映する */
function applyLayers(): void {
  terrain.sprite.visible = layers.terrain;
  plants.sprite.visible = layers.plants;
  carrion.sprite.visible = layers.carrion;
  herbivores.container.visible = layers.herbivores;
  carnivores.container.visible = layers.carnivores;
  border.visible = layers.border;
  lineageMark.visible = layers.lineage;
  markerLayer.visible = layers.marker;
  // 枠線の強調は、その種類を表示しているときだけ
  herbOutline.container.visible = layers.herbivores && highlightStyle === 'outline' && highlight === 'herbivore';
  carnOutline.container.visible = layers.carnivores && highlightStyle === 'outline' && highlight === 'carnivore';
  hud.setLayers(layers);
  try {
    localStorage.setItem(LAYERS_KEY, JSON.stringify(layers));
  } catch {
    // 覚えておけなくても表示は切り替わる
  }
}
/** 選び直す前の系統の統計が届くことがあるので、選んだ時刻より前の集計は使わない */
let lineageSince = 0;

/** 指定した種類を強調する（個体の選択とは独立）。方法は、もう一方を薄くするか、枠線を付けるか */
function setHighlight(target: Highlight, style: HighlightStyle): void {
  highlight = target;
  highlightStyle = style;
  const dim = style === 'dim';
  // 系統を選んでいる間は、系統の種類でないほうを薄くする（系統の外の個体は AgentLayer が薄く描く）
  const dimHerb = lineageSel ? lineageSel.kind !== HERBIVORE : dim && target === 'carnivore';
  const dimCarn = lineageSel ? lineageSel.kind !== CARNIVORE : dim && target === 'herbivore';
  herbivores.container.alpha = dimHerb ? DIMMED_ALPHA : 1;
  carnivores.container.alpha = dimCarn ? DIMMED_ALPHA : 1;
  herbOutline.container.visible = !dim && target === 'herbivore' && layers.herbivores;
  carnOutline.container.visible = !dim && target === 'carnivore' && layers.carnivores;
  hud.setHighlight(target, style);
}
/** 選択中の個体をカメラで追うか */
let following = false;
/**
 * 選択中の個体の id（0 は選択なし）。選び直した直後には、前の個体の情報が入った古いフレームが
 * 届くことがあるので、これと違う id の情報は無視する
 */
let selectedId = 0;

// 選択中の個体が死んだときの自動選び直し（config.ts の AUTO_RESELECT）
/** 予定しているタイマー。null なら予定なし */
let reselectTimer: number | null = null;
/** 自動選び直しを予定済みの、死んだ個体の id（同じ死亡で二重に予定しないため） */
let reselectScheduledFor = 0;
/** 自動選び直しの後も追従を続けるか（死んだ時点で追従していたか） */
let followAfterReselect = false;
/** 結果待ちの選択が、自動選び直しによるものか */
let pendingAutoSelect = false;
/** 結果待ちの選択が、系統からのランダムな選択によるものか（選べたら拡大して追う） */
let pendingRandomPick = false;
/** ランダムに選んだ個体を見るときの倍率 */
const PICK_SCALE = 3;

function cancelReselect(): void {
  if (reselectTimer !== null) {
    clearTimeout(reselectTimer);
    reselectTimer = null;
  }
  pendingAutoSelect = false;
  inspector.setNote('');
}

/** 死んだ場所 (x, y) の近くから、同じ種類の個体を少し待ってから選び直す */
function scheduleReselect(deadId: number, x: number, y: number, kind: number): void {
  if (reselectTimer !== null || reselectScheduledFor === deadId) return;
  reselectScheduledFor = deadId;
  followAfterReselect = following;
  inspector.setNote(`${AUTO_RESELECT.delayMs / 1000} 秒後に近くの個体に切り替えます`);
  reselectTimer = window.setTimeout(() => {
    reselectTimer = null;
    pendingAutoSelect = true;
    send({ type: 'select', x, y, radius: AUTO_RESELECT.radius, kind });
  }, AUTO_RESELECT.delayMs);
}

/** 系統を選ぶ（null で解除） */
function setLineage(sel: LineageSelection | null): void {
  lineageSel = sel;
  lineageSince = performance.now();
  send({ type: 'setLineage', lineage: sel });
  lineageHist.setSelected(sel);
  if (sel) lineagePanel.show(sel);
  else lineagePanel.hide();
  lineageMark.clear();
  setHighlight(highlight, highlightStyle);
}

/** 最後に表示した、選択中の個体の情報（「この系統を見る」で使う） */
let lastInspect: number[] | null = null;

const toast = new Toast(document.getElementById('toast')!);
const inspector = new Inspector(document.getElementById('inspector')!, {
  onClose: () => deselect(),
  onToggleFollow: () => setFollowing(!following),
  onSelectId: (id) => {
    cancelReselect();
    send({ type: 'selectId', id });
  },
  onShowLineage: () => {
    if (!lastInspect) return;
    // 標識色が近い（hue_similar 以内）個体を同じ系統とみなす。群れの指標と同じ定義
    const hue = lastInspect[INSPECT.hue];
    const w = SIM_PARAMS.hue_similar;
    setLineage({ kind: lastInspect[INSPECT.kind], from: (((hue - w) % 1) + 1) % 1, width: 2 * w });
  },
});

const camera = new Camera(world, app.canvas, params.world_width, params.world_height, {
  onClick: (px, py) => {
    if (camera.scale < MIN_SELECT_SCALE) {
      toast.show('拡大すると個体を選べます（ホイールで拡大）');
      return;
    }
    const { x, y } = camera.toWorld(px, py);
    cancelReselect();
    send({ type: 'select', x, y, radius: SELECT_RADIUS_PX / camera.scale });
  },
  // 自分で視点を動かしたら追従をやめる
  onPan: () => setFollowing(false),
});
camera.fit();
window.addEventListener('resize', () => camera.fit());

function setFollowing(on: boolean): void {
  following = on;
  inspector.setFollow(on);
}

function deselect(): void {
  cancelReselect();
  selectedId = 0;
  send({ type: 'deselect' });
  inspector.hide();
  marker.hide();
  setFollowing(false);
}

const stats = new StatsWindow(document.getElementById('stats')!, (open) => hud.setStatsOpen(open));
function toggleStats(): void {
  stats.toggle();
}
const brainWin = new BrainWindow(document.getElementById('brain')!, (open) => hud.setBrainOpen(open));

const hud = new Hud(document.getElementById('hud')!, {
  onTogglePause: () => setPaused(!paused),
  onToggleStats: toggleStats,
  onToggleBrain: () => brainWin.toggle(),
  onReset: () => start(),
  onSpeed: (s) => {
    speed = s;
    send({ type: 'setSpeed', speed });
    hud.setSpeed(speed);
  },
  onSunlight: (value) => {
    params.sunlight = value;
    send({ type: 'setParam', name: 'sunlight', value });
  },
  onAutoSunlight: (enabled) => send({ type: 'setAutoSunlight', enabled }),
  onHighlight: (target) => setHighlight(target, highlightStyle),
  onHighlightStyle: (style) => setHighlight(highlight, style),
  onMapMode: (mode: MapMode) => {
    terrain.setMode(mode);
    hud.setMapMode(mode);
    hud.setTempRange(mode === 'tempNow' ? terrain.tempNowRange : terrain.tempRange);
  },
  onPlantLayer: (layer) => {
    plantLayer = layer;
    plants.capacity = plantLayerMax(layer);
    send({ type: 'setPlantLayer', layer });
    hud.setPlantLayer(layer);
  },
  onLayer: (layer) => {
    layers[layer] = !layers[layer];
    applyLayers();
  },
  onColorMode: (mode) => {
    colorMode = mode;
    hud.setColorMode(mode);
  },
  onWorldSize: (size) => {
    if (size === sizeOf(params.world_width, params.world_height)) return;
    if (!window.confirm(`世界の広さを「${WORLD_SIZES[size].label}」にして、世界を作り直します。今の世界は失われます（必要なら先に保存してください）。`)) return;
    worldSize = size;
    try {
      localStorage.setItem(WORLD_SIZE_KEY, size);
    } catch {
      // 覚えておけなくても作り直せる
    }
    applyWorldSizeParams(size);
    start();
  },
  onSave: () => {
    toast.show('保存しています…');
    send({ type: 'save' });
  },
  onLoad: async (file) => {
    toast.show('読み込んでいます…');
    const buffer = await file.arrayBuffer();
    send({ type: 'load', buffer }, [buffer]);
  },
});

/** 世界を作り直したとき（リセット・読み込み）に、選択や表示の状態を戻す */
function resetView(): void {
  cancelReselect();
  setLineage(null);
  selectedId = 0;
  hud.clearHistory();
  inspector.hide();
  marker.hide();
  setFollowing(false);
}

function setPaused(p: boolean): void {
  paused = p;
  send({ type: 'setPaused', paused });
  hud.setPaused(paused);
}

/** 表示を世界の大きさに合わせる（枠・個体の層の範囲・カメラ） */
function applyWorldSize(width: number, height: number): void {
  border.clear().rect(0, 0, width, height).stroke({ width: 2, color: 0x56696f });
  for (const layer of [herbivores, carnivores, herbOutline, carnOutline]) layer.setWorldSize(width, height);
  camera.setWorld(width, height);
}

/** 選択肢の中から、この大きさの広さを探す（なければ null） */
function sizeOf(width: number, height: number): WorldSize | null {
  const found = (Object.keys(WORLD_SIZES) as WorldSize[]).find((k) => WORLD_SIZES[k].width === width && WORLD_SIZES[k].height === height);
  return found ?? null;
}

function start(): void {
  resetView();
  applyWorldSize(params.world_width, params.world_height);
  hud.setWorldSize(sizeOf(params.world_width, params.world_height));
  send({ type: 'init', seed: SEED, params });
}

function returnBuffer(frame: Frame): void {
  send({ type: 'returnBuffer', buffer: frame.buffer }, [frame.buffer]);
}

worker.onmessage = (e: MessageEvent<FromWorker>) => {
  const msg = e.data;
  switch (msg.type) {
    case 'ready':
      generation = msg.generation;
      stats.clear();
      terrain.setData(
        new Float32Array(msg.terrain),
        new Float32Array(msg.temperature),
        new Float32Array(msg.wood),
        new Float32Array(msg.elevation),
        msg.terrainCols,
        msg.terrainRows,
      );
      hud.setTempRange(terrain.currentMode === 'tempNow' ? terrain.tempNowRange : terrain.tempRange);
      break;
    case 'history':
      if (msg.generation === generation) stats.setHistory(msg);
      break;
    case 'selected':
      selectedId = msg.id;
      if (msg.id === 0) {
        toast.show(pendingAutoSelect ? '近くに代わりの個体がいません' : '近くに個体がいません');
        inspector.hide();
        marker.hide();
      }
      inspector.setNote('');
      if (pendingRandomPick && msg.id === 0) toast.show('この系統の個体はいません');
      // 自動選び直しなら、死んだ個体を追従していたときは新しい個体も追う。系統から選んだときは拡大して追う
      if (pendingRandomPick && msg.id !== 0 && camera.scale < PICK_SCALE) camera.zoomTo(PICK_SCALE);
      setFollowing(msg.id !== 0 && ((pendingAutoSelect && followAfterReselect) || pendingRandomPick));
      pendingAutoSelect = false;
      pendingRandomPick = false;
      break;
    case 'frame':
      if (msg.generation !== generation) return returnBuffer(msg); // リセット前の古いフレーム
      // まだ描画していない古いフレームは飛ばして、すぐ Worker に返す
      // 脳の重みは選んだ個体が変わったときの 1 フレームにしか入らないので、描画で飛ばされる前に取り込む
      if (msg.brainNet && msg.brainNetId !== undefined) {
        const b = msg.brain;
        const h1 = b?.h1 ?? 0;
        brainWin.setNet(msg.brainNetId, msg.brainNet, b?.input.length ?? 0, h1, (b?.hidden.length ?? 0) - h1, b?.output.length ?? 0);
      }
      if (latest) returnBuffer(latest);
      latest = msg;
      break;
    case 'saved': {
      // ダウンロードさせる
      const url = URL.createObjectURL(new Blob([msg.buffer], { type: 'application/gzip' }));
      const a = document.createElement('a');
      a.href = url;
      a.download = msg.filename;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
      const mb = (n: number) => (n / 1024 / 1024).toFixed(1);
      toast.show(`保存しました（${mb(msg.buffer.byteLength)} MB、圧縮前 ${mb(msg.bytes)} MB）`);
      break;
    }
    case 'loaded':
      if (!msg.ok) {
        toast.show(msg.message);
        break;
      }
      resetView();
      // 読み込んだ世界のパラメータを画面にも反映する（次のリセットでも同じ設定で作り直す）
      if (msg.params) Object.assign(params, msg.params);
      // 読み込んだ世界の広さに表示を合わせる
      applyWorldSize(params.world_width, params.world_height);
      hud.setWorldSize(sizeOf(params.world_width, params.world_height));
      hud.setSunlight(params.sunlight);
      toast.show(`tick ${msg.tick.toLocaleString('ja-JP')} の世界を読み込みました${msg.message ? `。${msg.message}` : ''}`);
      break;
    case 'climate':
      if (msg.generation !== generation) break;
      terrain.setClimate(new Float32Array(msg.temperature), new Float32Array(msg.fertility));
      break;
    case 'stats':
      hud.setStats(msg, speed, paused);
      if (msg.lineage && lineageSel && performance.now() - lineageSince > 100) showLineage(msg.lineage);
      break;
  }
};

app.ticker.add(() => {
  if (!latest) return;
  const f = latest;
  const agentFloats = f.agentCount * f.stride;
  const agents = new Float32Array(f.buffer, 0, agentFloats);
  // 強調中の種類は、画面上で最低 HIGHLIGHT_MIN_RADIUS_PX の半径で描く（テクスチャの半径は草食の半径）
  const minScale = HIGHLIGHT_MIN_RADIUS_PX / camera.scale / BODY_RADIUS.herbivore;
  // 強調していなくても、画面上で MIN_BODY_RADIUS_PX より小さくは描かない（小さな種を見失わないように）
  const baseMin = MIN_BODY_RADIUS_PX / camera.scale / BODY_RADIUS.herbivore;
  const herbMin = highlight === 'herbivore' ? minScale : baseMin;
  const carnMin = highlight === 'carnivore' ? minScale : baseMin;
  herbivores.apply(agents, f.agentCount, f.stride, HERBIVORE, colorMode, herbMin, lineageSel);
  carnivores.apply(agents, f.agentCount, f.stride, CARNIVORE, colorMode, carnMin, lineageSel);
  // 枠線は表示中のときだけ更新する（計算の節約）
  if (herbOutline.container.visible) herbOutline.apply(agents, f.agentCount, f.stride, HERBIVORE, 'kind', herbMin);
  if (carnOutline.container.visible) carnOutline.apply(agents, f.agentCount, f.stride, CARNIVORE, 'kind', carnMin);
  const cells = f.plantCols * f.plantRows;
  plants.apply(new Float32Array(f.buffer, agentFloats * 4, cells), f.plantCols, f.plantRows);
  carrion.apply(new Float32Array(f.buffer, (agentFloats + cells) * 4, cells), f.plantCols, f.plantRows);
  const now = performance.now();
  if (now - lastLineage > LINEAGE_INTERVAL_MS) {
    lineageHist.update(agents, f.agentCount, f.stride);
    lastLineage = now;
  }
  hud.setTick(f.tick);
  brainWin.update(f.brain, f.inspect, f.tick);
  if (f.inspect) {
    showSelection(f.inspect);
    if (f.inspect[INSPECT.status] === 1 && f.inspect[INSPECT.id] === selectedId) {
      inspector.updateExtra(f.brain, f.energyHistory, f.inspect[INSPECT.reproThreshold]);
    }
  }
  returnBuffer(f);
  latest = null;
});

/** 系統の統計を表示し、地図に重心と広がりを描く */
function showLineage(stats: number[]): void {
  lineagePanel.update(stats);
  lineageMark.clear();
  if (stats[LINEAGE.count] === 0) return;
  const x = stats[LINEAGE.centerX];
  const y = stats[LINEAGE.centerY];
  // 線の太さは画面上で一定にする
  const width = 2 / camera.scale;
  const arm = 12 / camera.scale;
  lineageMark
    .circle(x, y, stats[LINEAGE.spread])
    .stroke({ width, color: COLORS.marker, alpha: 0.8 })
    .moveTo(x - arm, y)
    .lineTo(x + arm, y)
    .moveTo(x, y - arm)
    .lineTo(x, y + arm)
    .stroke({ width, color: COLORS.marker });
}

function showSelection(v: number[]): void {
  const status = v[INSPECT.status];
  if (status === 0) return; // 選択直後で、まだ結果が来ていない
  if (v[INSPECT.id] !== selectedId) return; // 選び直す前の個体の情報（古いフレーム）
  if (status === 1) lastInspect = v;
  inspector.update(v);
  if (status === 2) {
    marker.hide(); // 死んだ個体はもういない
    // 位置と種類は死ぬ直前の値が残っている
    scheduleReselect(v[INSPECT.id], v[INSPECT.x], v[INSPECT.y], v[INSPECT.kind]);
    setFollowing(false);
    return;
  }
  const x = v[INSPECT.x];
  const y = v[INSPECT.y];
  // いまの大きさ = 大人の大きさ × 成長度
  const radius =
    (v[INSPECT.kind] === CARNIVORE ? BODY_RADIUS.carnivore : BODY_RADIUS.herbivore) * v[INSPECT.size] * v[INSPECT.growth];
  if (following) camera.centerOn(x, y);
  marker.update(x, y, radius, v[INSPECT.vision], camera.scale);
}

setInterval(() => hud.setFps(app.ticker.FPS), 250);

const lineageHist = new LineageHistogram(...hud.lineageTargets(), (sel) => setLineage(sel));
const lineagePanel = new LineagePanel(
  hud.lineagePanelRoot(),
  () => setLineage(null),
  () => {
    if (!lineageSel) return;
    cancelReselect();
    pendingRandomPick = true;
    send({ type: 'selectRandomInLineage', lineage: lineageSel });
  },
);
/** 系統色のヒストグラムを描き直す間隔（ms）。毎フレーム描く必要はない */
const LINEAGE_INTERVAL_MS = 500;
let lastLineage = 0;

window.addEventListener('keydown', (e) => {
  if (e.code === 'Space' && !(e.target instanceof HTMLButtonElement || e.target instanceof HTMLInputElement)) {
    e.preventDefault();
    setPaused(!paused);
  }
  if (e.code === 'KeyG' && !e.ctrlKey && !e.metaKey && !e.altKey && !(e.target instanceof HTMLInputElement)) toggleStats();
  if (e.code === 'KeyB' && !e.ctrlKey && !e.metaKey && !e.altKey && !(e.target instanceof HTMLInputElement)) brainWin.toggle();
});

hud.setSpeed(speed);
hud.setPaused(paused);
hud.setSunlight(params.sunlight);
hud.setColorMode(colorMode);
setHighlight('none', 'dim');
hud.setMapMode('vegetation');
hud.setPlantLayer(plantLayer);
applyLayers();
start();

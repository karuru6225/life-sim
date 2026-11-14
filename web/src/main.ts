import { Application, Container, Graphics } from 'pixi.js';
import { AUTO_RESELECT, BODY_RADIUS, COLORS, DIMMED_ALPHA, HIGHLIGHT_MIN_RADIUS_PX, MIN_SELECT_SCALE, SEED, SELECT_RADIUS_PX, SIM_PARAMS, type SimParams } from './config';
import { CARNIVORE, HERBIVORE, INSPECT, type FromWorker, type ToWorker } from './protocol';
import { AgentLayer, type ColorMode } from './render/agents';
import { Camera } from './render/camera';
import { PlantLayer } from './render/plants';
import { TerrainLayer } from './render/terrain';
import { SelectionMarker } from './render/selection';
import { Hud, type Highlight, type HighlightStyle } from './ui/hud';
import { Inspector, Toast } from './ui/inspector';
import './style.css';

type Frame = Extract<FromWorker, { type: 'frame' }>;

const params: SimParams = { ...SIM_PARAMS };

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
world.addChild(terrain.sprite, plants.sprite);

// 世界の外枠。トーラスなので端はつながっているが、どこまでが世界かは見せておく
world.addChild(
  new Graphics().rect(0, 0, params.world_width, params.world_height).stroke({ width: 2, color: 0x56696f }),
);

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
world.addChild(herbivores.container, carnivores.container, herbOutline.container, carnOutline.container, marker.graphics);

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker, transfer: Transferable[] = []) => worker.postMessage(msg, transfer);

let generation = -1;
let latest: Frame | null = null;
let speed = 1;
let paused = false;
let colorMode: ColorMode = 'kind';
let highlight: Highlight = 'none';
let highlightStyle: HighlightStyle = 'dim';

/** 指定した種類を強調する（個体の選択とは独立）。方法は、もう一方を薄くするか、枠線を付けるか */
function setHighlight(target: Highlight, style: HighlightStyle): void {
  highlight = target;
  highlightStyle = style;
  const dim = style === 'dim';
  herbivores.container.alpha = dim && target === 'carnivore' ? DIMMED_ALPHA : 1;
  carnivores.container.alpha = dim && target === 'herbivore' ? DIMMED_ALPHA : 1;
  herbOutline.container.visible = !dim && target === 'herbivore';
  carnOutline.container.visible = !dim && target === 'carnivore';
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

const toast = new Toast(document.getElementById('toast')!);
const inspector = new Inspector(document.getElementById('inspector')!, {
  onClose: () => deselect(),
  onToggleFollow: () => setFollowing(!following),
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

const hud = new Hud(document.getElementById('hud')!, {
  onTogglePause: () => setPaused(!paused),
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
  onColorMode: (mode) => {
    colorMode = mode;
    hud.setColorMode(mode);
  },
});

function setPaused(p: boolean): void {
  paused = p;
  send({ type: 'setPaused', paused });
  hud.setPaused(paused);
}

function start(): void {
  cancelReselect();
  selectedId = 0;
  hud.clearHistory();
  inspector.hide();
  marker.hide();
  setFollowing(false);
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
      terrain.apply(new Float32Array(msg.terrain), msg.terrainCols, msg.terrainRows);
      break;
    case 'selected':
      selectedId = msg.id;
      if (msg.id === 0) {
        toast.show(pendingAutoSelect ? '近くに代わりの個体がいません' : '近くに個体がいません');
        inspector.hide();
        marker.hide();
      }
      inspector.setNote('');
      // 自動選び直しなら、死んだ個体を追従していたときは新しい個体も追う
      setFollowing(pendingAutoSelect && followAfterReselect && msg.id !== 0);
      pendingAutoSelect = false;
      break;
    case 'frame':
      if (msg.generation !== generation) return returnBuffer(msg); // リセット前の古いフレーム
      // まだ描画していない古いフレームは飛ばして、すぐ Worker に返す
      if (latest) returnBuffer(latest);
      latest = msg;
      break;
    case 'stats':
      hud.setStats(msg, speed, paused);
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
  const herbMin = highlight === 'herbivore' ? minScale : 0;
  const carnMin = highlight === 'carnivore' ? minScale : 0;
  herbivores.apply(agents, f.agentCount, f.stride, HERBIVORE, colorMode, herbMin);
  carnivores.apply(agents, f.agentCount, f.stride, CARNIVORE, colorMode, carnMin);
  // 枠線は表示中のときだけ更新する（計算の節約）
  if (herbOutline.container.visible) herbOutline.apply(agents, f.agentCount, f.stride, HERBIVORE, 'kind', herbMin);
  if (carnOutline.container.visible) carnOutline.apply(agents, f.agentCount, f.stride, CARNIVORE, 'kind', carnMin);
  plants.apply(new Float32Array(f.buffer, agentFloats * 4, f.plantCols * f.plantRows), f.plantCols, f.plantRows);
  hud.setTick(f.tick);
  if (f.inspect) showSelection(f.inspect);
  returnBuffer(f);
  latest = null;
});

function showSelection(v: number[]): void {
  const status = v[INSPECT.status];
  if (status === 0) return; // 選択直後で、まだ結果が来ていない
  if (v[INSPECT.id] !== selectedId) return; // 選び直す前の個体の情報（古いフレーム）
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

window.addEventListener('keydown', (e) => {
  if (e.code === 'Space' && !(e.target instanceof HTMLButtonElement || e.target instanceof HTMLInputElement)) {
    e.preventDefault();
    setPaused(!paused);
  }
});

hud.setSpeed(speed);
hud.setPaused(paused);
hud.setSunlight(params.sunlight);
hud.setColorMode(colorMode);
setHighlight('none', 'dim');
start();

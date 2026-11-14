import { Application, Container, Graphics } from 'pixi.js';
import { BODY_RADIUS, COLORS, MIN_SELECT_SCALE, SEED, SELECT_RADIUS_PX, SIM_PARAMS, type SimParams } from './config';
import { CARNIVORE, HERBIVORE, INSPECT, type FromWorker, type ToWorker } from './protocol';
import { AgentLayer, type ColorMode } from './render/agents';
import { Camera } from './render/camera';
import { PlantLayer } from './render/plants';
import { SelectionMarker } from './render/selection';
import { Hud } from './ui/hud';
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

const plants = new PlantLayer(params.plant_cell, params.plant_capacity);
world.addChild(plants.sprite);

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
// テクスチャは草食の半径で 1 つだけ作り、大きさは個体ごとの倍率で表す
const body = dot(BODY_RADIUS.herbivore);
const herbivores = new AgentLayer(body, COLORS.herbivore, 1, params.world_width, params.world_height);
// 肉食は数が少なく埋もれやすいので、一回り大きくして上に重ねる
const carnivores = new AgentLayer(
  body,
  COLORS.carnivore,
  BODY_RADIUS.carnivore / BODY_RADIUS.herbivore,
  params.world_width,
  params.world_height,
);
const marker = new SelectionMarker();
world.addChild(herbivores.container, carnivores.container, marker.graphics);

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker, transfer: Transferable[] = []) => worker.postMessage(msg, transfer);

let generation = -1;
let latest: Frame | null = null;
let speed = 1;
let paused = false;
let colorMode: ColorMode = 'kind';
/** 選択中の個体をカメラで追うか */
let following = false;

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
      break;
    case 'selected':
      if (msg.id === 0) {
        toast.show('近くに個体がいません');
        inspector.hide();
        marker.hide();
      }
      setFollowing(false);
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
  herbivores.apply(agents, f.agentCount, f.stride, HERBIVORE, colorMode);
  carnivores.apply(agents, f.agentCount, f.stride, CARNIVORE, colorMode);
  plants.apply(new Float32Array(f.buffer, agentFloats * 4, f.plantCols * f.plantRows), f.plantCols, f.plantRows);
  hud.setTick(f.tick);
  if (f.inspect) showSelection(f.inspect);
  returnBuffer(f);
  latest = null;
});

function showSelection(v: number[]): void {
  const status = v[INSPECT.status];
  if (status === 0) return; // 選択直後で、まだ結果が来ていない
  inspector.update(v);
  if (status === 2) {
    marker.hide(); // 死んだ個体はもういない
    setFollowing(false);
    return;
  }
  const x = v[INSPECT.x];
  const y = v[INSPECT.y];
  const radius = (v[INSPECT.kind] === CARNIVORE ? BODY_RADIUS.carnivore : BODY_RADIUS.herbivore) * v[INSPECT.size];
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
start();

import { Application, Container, Graphics } from 'pixi.js';
import { COLORS, SEED, SIM_PARAMS, type SimParams } from './config';
import type { FromWorker, ToWorker } from './protocol';
import { AgentLayer } from './render/agents';
import { Camera } from './render/camera';
import { PlantLayer } from './render/plants';
import { Hud } from './ui/hud';
import './style.css';

type Frame = Extract<FromWorker, { type: 'frame' }>;

const HERBIVORE = 0;
const CARNIVORE = 1;

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
const herbivores = new AgentLayer(dot(3), COLORS.herbivore, params.world_width, params.world_height);
// 肉食は数が少なく埋もれやすいので、一回り大きくして上に重ねる
const carnivores = new AgentLayer(dot(4.5), COLORS.carnivore, params.world_width, params.world_height);
world.addChild(herbivores.container, carnivores.container);

const camera = new Camera(world, app.canvas, params.world_width, params.world_height);
camera.fit();
window.addEventListener('resize', () => camera.fit());

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker, transfer: Transferable[] = []) => worker.postMessage(msg, transfer);

let generation = -1;
let latest: Frame | null = null;
let speed = 1;
let paused = false;

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
});

function setPaused(p: boolean): void {
  paused = p;
  send({ type: 'setPaused', paused });
  hud.setPaused(paused);
}

function start(): void {
  hud.clearHistory();
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
  herbivores.apply(agents, f.agentCount, f.stride, HERBIVORE);
  carnivores.apply(agents, f.agentCount, f.stride, CARNIVORE);
  plants.apply(new Float32Array(f.buffer, agentFloats * 4, f.plantCols * f.plantRows), f.plantCols, f.plantRows);
  hud.setTick(f.tick);
  returnBuffer(f);
  latest = null;
});

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
start();

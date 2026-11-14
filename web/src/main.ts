import { Application, Container, Graphics } from 'pixi.js';
import { COLORS, DEFAULT_COUNT, WORLD } from './config';
import type { FromWorker, ToWorker } from './protocol';
import { AgentLayer } from './render/agents';
import { Camera } from './render/camera';
import { Hud } from './ui/hud';
import './style.css';

type Frame = Extract<FromWorker, { type: 'frame' }>;

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

// 世界の外枠。トーラスなので端はつながっているが、どこまでが世界かは見せておく
world.addChild(new Graphics().rect(0, 0, WORLD.width, WORLD.height).stroke({ width: 2, color: 0x56696f }));

const dotTexture = app.renderer.generateTexture(new Graphics().circle(0, 0, 3).fill(0xffffff));
const agents = new AgentLayer(dotTexture, WORLD.width, WORLD.height);
world.addChild(agents.container);

const camera = new Camera(world, app.canvas, WORLD.width, WORLD.height);
camera.fit();
window.addEventListener('resize', () => camera.fit());

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker, transfer: Transferable[] = []) => worker.postMessage(msg, transfer);

let generation = -1;
let stride = 3;
let latest: Frame | null = null;
let speed = 1;
let paused = false;

const hud = new Hud(document.getElementById('hud')!, {
  onTogglePause: () => setPaused(!paused),
  onSpeed: (s) => {
    speed = s;
    send({ type: 'setSpeed', speed });
    hud.setSpeed(speed);
  },
  onCount: (count) => start(count),
});

function setPaused(p: boolean): void {
  paused = p;
  send({ type: 'setPaused', paused });
  hud.setPaused(paused);
}

function start(count: number): void {
  send({ type: 'init', seed: WORLD.seed, count, width: WORLD.width, height: WORLD.height });
}

function returnBuffer(frame: Frame): void {
  send({ type: 'returnBuffer', buffer: frame.buffer }, [frame.buffer]);
}

worker.onmessage = (e: MessageEvent<FromWorker>) => {
  const msg = e.data;
  switch (msg.type) {
    case 'ready':
      generation = msg.generation;
      stride = msg.stride;
      agents.resize(msg.count);
      hud.setCount(msg.count);
      break;
    case 'frame':
      if (msg.generation !== generation) return; // 個体数を変える前の古いフレーム
      // まだ描画していない古いフレームは飛ばして、すぐ Worker に返す
      if (latest) returnBuffer(latest);
      latest = msg;
      break;
    case 'stats':
      hud.setSimStats(msg.ticksPerSecond, msg.msPerTick, speed, paused);
      break;
  }
};

app.ticker.add(() => {
  if (!latest) return;
  agents.apply(new Float32Array(latest.buffer), latest.count, stride);
  hud.setTick(latest.tick);
  returnBuffer(latest);
  latest = null;
});

setInterval(() => hud.setFps(app.ticker.FPS), 250);

window.addEventListener('keydown', (e) => {
  if (e.code === 'Space' && !(e.target instanceof HTMLButtonElement)) {
    e.preventDefault();
    setPaused(!paused);
  }
});

// ?n=100000 のように URL で個体数を指定できる（ベンチマーク用）
const requested = Number(new URLSearchParams(location.search).get('n'));
hud.setSpeed(speed);
hud.setPaused(paused);
start(Number.isFinite(requested) && requested > 0 ? Math.floor(requested) : DEFAULT_COUNT);

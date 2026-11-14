// 計測値と操作のパネル。Phase 0 は性能計測が目的なので、描画 fps と 1 tick の計算時間を主役にする。

import { COUNT_OPTIONS, DENSITY_LEVELS, SPEEDS, TICK_HZ } from '../config';
import { densityRamp } from '../render/agents';

export interface HudHandlers {
  onTogglePause(): void;
  onSpeed(speed: number): void;
  onCount(count: number): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

export class Hud {
  private readonly fps: HTMLElement;
  private readonly msPerTick: HTMLElement;
  private readonly tps: HTMLElement;
  private readonly budget: HTMLElement;
  private readonly count: HTMLElement;
  private readonly tick: HTMLElement;
  private readonly pauseButton: HTMLButtonElement;
  private readonly speedButtons: HTMLButtonElement[] = [];
  private readonly countButtons: HTMLButtonElement[] = [];

  constructor(root: HTMLElement, handlers: HudHandlers) {
    root.innerHTML = `
      <div class="hud-title">life-sim <span>phase 0 · 性能計測</span></div>
      <div class="hud-primary">
        <div class="metric"><span class="value" data-k="fps">–</span><span class="unit">fps</span><span class="label">描画</span></div>
        <div class="metric"><span class="value" data-k="ms">–</span><span class="unit">ms/tick</span><span class="label">計算</span></div>
      </div>
      <dl class="hud-rows">
        <dt>個体</dt><dd data-k="count">–</dd>
        <dt>速度</dt><dd><span data-k="tps">–</span> tick/s</dd>
        <dt>余裕</dt><dd data-k="budget">–</dd>
        <dt>tick</dt><dd data-k="tick">–</dd>
      </dl>
      <div class="hud-controls">
        <button type="button" class="pause" data-k="pause">一時停止</button>
        <div class="group" data-k="speeds"><span class="group-label">速度</span></div>
        <div class="group" data-k="counts"><span class="group-label">個体数</span></div>
      </div>
      <div class="hud-legend">
        <span class="group-label">近傍 16px 内の個体数</span>
        <div class="swatches" data-k="swatches"></div>
        <div class="swatch-scale"><span>0</span><span>${DENSITY_LEVELS - 1}+</span></div>
      </div>
      <p class="hud-help">ドラッグ: 移動 · ホイール: ズーム · ダブルクリック: 全体 · Space: 一時停止</p>
    `;
    const q = <T extends HTMLElement>(k: string) => root.querySelector<T>(`[data-k="${k}"]`)!;
    this.fps = q('fps');
    this.msPerTick = q('ms');
    this.tps = q('tps');
    this.budget = q('budget');
    this.count = q('count');
    this.tick = q('tick');
    this.pauseButton = q<HTMLButtonElement>('pause');
    this.pauseButton.addEventListener('click', handlers.onTogglePause);

    for (const speed of SPEEDS) {
      const b = button(speed === Infinity ? '最大' : `${speed}×`, () => handlers.onSpeed(speed));
      b.dataset.value = String(speed);
      this.speedButtons.push(b);
      q('speeds').append(b);
    }
    for (const count of COUNT_OPTIONS) {
      const b = button(`${count / 10_000}万`, () => handlers.onCount(count));
      b.dataset.value = String(count);
      this.countButtons.push(b);
      q('counts').append(b);
    }
    for (const color of densityRamp()) {
      const s = document.createElement('span');
      s.style.background = color;
      q('swatches').append(s);
    }
  }

  setFps(fps: number): void {
    this.fps.textContent = fps.toFixed(0);
  }

  setSimStats(ticksPerSecond: number, msPerTick: number, speed: number, paused: boolean): void {
    this.tps.textContent = ticksPerSecond.toFixed(1);
    this.msPerTick.textContent = msPerTick > 0 ? msPerTick.toFixed(2) : '–';
    // 等倍の 1 tick に使える時間に対して、実際の計算時間がどれだけか
    if (msPerTick > 0) {
      const maxTps = 1000 / msPerTick;
      this.budget.textContent = `${fmtInt.format(Math.round(maxTps))} tick/s まで（${(maxTps / TICK_HZ).toFixed(1)}×）`;
    }
    const target = TICK_HZ * speed;
    const lagging = !paused && speed !== Infinity && ticksPerSecond < target * 0.9;
    this.tps.classList.toggle('warn', lagging);
  }

  setCount(count: number): void {
    this.count.textContent = fmtInt.format(count);
    for (const b of this.countButtons) b.classList.toggle('active', Number(b.dataset.value) === count);
  }

  setTick(tick: number): void {
    this.tick.textContent = fmtInt.format(tick);
  }

  setSpeed(speed: number): void {
    for (const b of this.speedButtons) b.classList.toggle('active', Number(b.dataset.value) === speed);
  }

  setPaused(paused: boolean): void {
    this.pauseButton.textContent = paused ? '再開' : '一時停止';
    this.pauseButton.classList.toggle('active', paused);
  }
}

function button(label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = label;
  b.addEventListener('click', onClick);
  return b;
}

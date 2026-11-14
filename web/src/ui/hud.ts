// 観察パネル。Phase 1 は個体数の推移が主役。性能の数値は脇に置く。

import { COLORS, HISTORY_LENGTH, SPEEDS, SUNLIGHT_RANGE, TICK_HZ } from '../config';
import type { FromWorker } from '../protocol';

type Stats = Extract<FromWorker, { type: 'stats' }>;

export interface HudHandlers {
  onTogglePause(): void;
  onReset(): void;
  onSpeed(speed: number): void;
  onSunlight(value: number): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

export class Hud {
  private readonly el: Record<string, HTMLElement> = {};
  private readonly speedButtons: HTMLButtonElement[] = [];
  private readonly chart: PopulationChart;

  constructor(root: HTMLElement, handlers: HudHandlers) {
    root.innerHTML = `
      <div class="hud-title">life-sim <span>phase 1 · 草食と肉食</span></div>
      <div class="hud-primary">
        <div class="metric">
          <span class="value" data-k="herb">–</span>
          <span class="label"><i class="dot" style="background:${COLORS.herbivore}"></i>草食</span>
        </div>
        <div class="metric">
          <span class="value" data-k="carn">–</span>
          <span class="label"><i class="dot dot-large" style="background:${COLORS.carnivore}"></i>肉食</span>
        </div>
      </div>
      <canvas class="hud-chart" data-k="chart" aria-label="直近 2 分の個体数の推移"></canvas>
      <div class="chart-caption">直近 2 分 · 線ごとに縦軸の範囲が違う</div>
      <dl class="hud-rows">
        <dt>植物量</dt><dd data-k="plant">–</dd>
        <dt>日射量</dt>
        <dd class="slider-row">
          <input type="range" data-k="sun" min="${SUNLIGHT_RANGE.min}" max="${SUNLIGHT_RANGE.max}" step="${SUNLIGHT_RANGE.step}" />
          <span data-k="sunValue">–</span>
        </dd>
      </dl>
      <div class="hud-controls">
        <div class="group">
          <button type="button" data-k="pause">一時停止</button>
          <button type="button" data-k="reset">リセット</button>
        </div>
        <div class="group" data-k="speeds"><span class="group-label">速度</span></div>
      </div>
      <dl class="hud-rows hud-perf">
        <dt>描画</dt><dd><span data-k="fps">–</span> fps</dd>
        <dt>計算</dt><dd><span data-k="ms">–</span> ms/tick</dd>
        <dt>速度</dt><dd><span data-k="tps">–</span> tick/s</dd>
        <dt>余裕</dt><dd data-k="budget">–</dd>
        <dt>tick</dt><dd data-k="tick">–</dd>
      </dl>
      <div class="hud-legend">
        <span class="group-label">植物量</span>
        <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.background}, ${COLORS.plant})"></div>
        <div class="swatch-scale"><span>なし</span><span>上限</span></div>
      </div>
      <p class="hud-help">ドラッグ: 移動 · ホイール: ズーム · ダブルクリック: 全体 · Space: 一時停止</p>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) {
      this.el[node.dataset.k!] = node;
    }

    this.el.pause.addEventListener('click', handlers.onTogglePause);
    this.el.reset.addEventListener('click', handlers.onReset);
    const sun = this.el.sun as HTMLInputElement;
    sun.addEventListener('input', () => {
      this.el.sunValue.textContent = Number(sun.value).toFixed(3);
      handlers.onSunlight(Number(sun.value));
    });

    for (const speed of SPEEDS) {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = speed === Infinity ? '最大' : `${speed}×`;
      b.dataset.value = String(speed);
      b.addEventListener('click', () => handlers.onSpeed(speed));
      this.speedButtons.push(b);
      this.el.speeds.append(b);
    }

    this.chart = new PopulationChart(this.el.chart as HTMLCanvasElement);
  }

  setFps(fps: number): void {
    this.el.fps.textContent = fps.toFixed(0);
  }

  setStats(s: Stats, speed: number, paused: boolean): void {
    this.el.herb.textContent = fmtInt.format(s.herbivores);
    this.el.carn.textContent = fmtInt.format(s.carnivores);
    this.el.plant.textContent = fmtInt.format(Math.round(s.plantTotal));

    this.el.tps.textContent = s.ticksPerSecond.toFixed(1);
    this.el.ms.textContent = s.msPerTick > 0 ? s.msPerTick.toFixed(2) : '–';
    if (s.msPerTick > 0) {
      const maxTps = 1000 / s.msPerTick;
      this.el.budget.textContent = `${fmtInt.format(Math.round(maxTps))} tick/s まで（${(maxTps / TICK_HZ).toFixed(1)}×）`;
    }
    const lagging = !paused && speed !== Infinity && s.ticksPerSecond < TICK_HZ * speed * 0.9;
    this.el.tps.classList.toggle('warn', lagging);

    if (!paused) this.chart.push(s.herbivores, s.carnivores);
  }

  clearHistory(): void {
    this.chart.clear();
  }

  setTick(tick: number): void {
    this.el.tick.textContent = fmtInt.format(tick);
  }

  setSpeed(speed: number): void {
    for (const b of this.speedButtons) b.classList.toggle('active', Number(b.dataset.value) === speed);
  }

  setPaused(paused: boolean): void {
    this.el.pause.textContent = paused ? '再開' : '一時停止';
    this.el.pause.classList.toggle('active', paused);
  }

  setSunlight(value: number): void {
    (this.el.sun as HTMLInputElement).value = String(value);
    this.el.sunValue.textContent = value.toFixed(3);
  }
}

/**
 * 草食と肉食の個体数の推移。数が 1 桁違うので、線ごとに自分の最小〜最大で縦軸を取る
 * （絶対値は上の数字が担当し、グラフは増減の形を見せる）。
 */
class PopulationChart {
  private herb: number[] = [];
  private carn: number[] = [];
  private readonly ctx: CanvasRenderingContext2D;

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext('2d')!;
  }

  push(herb: number, carn: number): void {
    this.herb.push(herb);
    this.carn.push(carn);
    if (this.herb.length > HISTORY_LENGTH) {
      this.herb.shift();
      this.carn.shift();
    }
    this.draw();
  }

  clear(): void {
    this.herb = [];
    this.carn = [];
    this.draw();
  }

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (this.canvas.width !== Math.round(w * dpr)) {
      this.canvas.width = Math.round(w * dpr);
      this.canvas.height = Math.round(h * dpr);
    }
    const ctx = this.ctx;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    this.line(this.herb, COLORS.herbivore, w, h);
    this.line(this.carn, COLORS.carnivore, w, h);
  }

  private line(values: number[], color: string, w: number, h: number): void {
    if (values.length < 2) return;
    let min = Math.min(...values);
    let max = Math.max(...values);
    // 変動が小さいときに、わずかな揺れが画面いっぱいの山に化けないよう、最低限の幅を持たせる
    const minSpan = Math.max(1, max * 0.2);
    if (max - min < minSpan) {
      const mid = (max + min) / 2;
      min = mid - minSpan / 2;
      max = mid + minSpan / 2;
    }
    const pad = 3;
    const ctx = this.ctx;
    ctx.beginPath();
    values.forEach((v, i) => {
      const x = (i / (HISTORY_LENGTH - 1)) * w;
      const y = pad + (1 - (v - min) / (max - min)) * (h - pad * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }
}

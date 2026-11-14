// 観察パネル。個体数の推移と形質の平均（＝進化の進み具合）が主役。性能の数値は脇に置く。

import { COLORS, HISTORY_LENGTH, SPEEDS, SUNLIGHT_RANGE, TICK_HZ } from '../config';
import type { FromWorker, TraitMeans } from '../protocol';
import type { ColorMode } from '../render/agents';
import { HUE_PALETTE } from '../render/palette';

type Stats = Extract<FromWorker, { type: 'stats' }>;

export interface HudHandlers {
  onTogglePause(): void;
  onReset(): void;
  onSpeed(speed: number): void;
  onSunlight(value: number): void;
  onColorMode(mode: ColorMode): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

/** 形質の表の行: [ラベル, 値の取り出し方, 小数の桁数] */
const TRAIT_ROWS: [string, (m: TraitMeans) => number, number][] = [
  ['大きさ', (m) => m.size, 2],
  ['最高速度', (m) => m.speed, 2],
  ['視野', (m) => m.vision, 1],
  ['子への投資', (m) => m.invest, 2],
  ['世代', (m) => m.generation, 0],
  ['集まり具合', (m) => m.clustering, 2],
  ['向きのそろい', (m) => m.alignment, 2],
];

export class Hud {
  private readonly el: Record<string, HTMLElement> = {};
  private readonly speedButtons: HTMLButtonElement[] = [];
  private readonly modeButtons: HTMLButtonElement[] = [];
  private readonly traitCells: HTMLElement[][] = [];
  private readonly chart: PopulationChart;

  constructor(root: HTMLElement, handlers: HudHandlers) {
    root.innerHTML = `
      <div class="hud-title">life-sim <span>phase 2 · 遺伝と進化</span></div>
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
      <table class="trait-table">
        <caption>形質の平均と群れの度合い</caption>
        <thead><tr><th></th><th>草食</th><th>肉食</th></tr></thead>
        <tbody data-k="traits"></tbody>
      </table>
      <dl class="hud-rows">
        <dt>植物量</dt><dd data-k="plant">–</dd>
        <dt>日射量</dt>
        <dd class="slider-row">
          <input type="range" data-k="sun" min="${SUNLIGHT_RANGE.min}" max="${SUNLIGHT_RANGE.max}" step="${SUNLIGHT_RANGE.step}" aria-label="日射量" />
          <span data-k="sunValue">–</span>
        </dd>
      </dl>
      <div class="hud-controls">
        <div class="group">
          <button type="button" data-k="pause">一時停止</button>
          <button type="button" data-k="reset">リセット</button>
        </div>
        <div class="group" data-k="speeds"><span class="group-label">速度</span></div>
        <div class="group" data-k="modes"><span class="group-label">色分け</span></div>
      </div>
      <dl class="hud-rows hud-perf">
        <dt>描画</dt><dd><span data-k="fps">–</span> fps</dd>
        <dt>計算</dt><dd><span data-k="ms">–</span> ms/tick</dd>
        <dt>速度</dt><dd><span data-k="tps">–</span> tick/s</dd>
        <dt>余裕</dt><dd data-k="budget">–</dd>
        <dt>tick</dt><dd data-k="tick">–</dd>
      </dl>
      <div class="hud-legend">
        <div data-k="lineageLegend" hidden>
          <span class="group-label">系統色（標識色の遺伝子）</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${HUE_PALETTE.join(', ')})"></div>
          <p class="legend-note">生存に影響しない遺伝子で、親から子へ少しずつずれながら受け継がれる。近い色ほど近い系統</p>
        </div>
        <span class="group-label">植物量</span>
        <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.background}, ${COLORS.plant})"></div>
        <div class="swatch-scale"><span>なし</span><span>上限</span></div>
        <p class="legend-note">点の大きさは「大きさ」の遺伝子に比例。肉食は見やすさのため 1.5 倍で表示</p>
        <p class="legend-note">集まり具合: 最寄りの仲間までの距離 ÷ ランダムに散らばった場合の期待値（1 でランダム、小さいほど集まっている）。向きのそろい: 近くの仲間との向きの一致度（0 でばらばら、1 で同じ向き）</p>
      </div>
      <p class="hud-help">ドラッグ: 移動 · ホイール: ズーム · ダブルクリック: 全体 · 拡大してクリック: 個体を選択 · Space: 一時停止</p>
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
      const b = button(speed === Infinity ? '最大' : `${speed}×`, () => handlers.onSpeed(speed));
      b.dataset.value = String(speed);
      this.speedButtons.push(b);
      this.el.speeds.append(b);
    }
    for (const [mode, label] of [
      ['kind', '種類'],
      ['lineage', '系統'],
    ] as const) {
      const b = button(label, () => handlers.onColorMode(mode));
      b.dataset.value = mode;
      this.modeButtons.push(b);
      this.el.modes.append(b);
    }

    for (const [label] of TRAIT_ROWS) {
      const tr = document.createElement('tr');
      const th = document.createElement('th');
      th.textContent = label;
      const cells = [document.createElement('td'), document.createElement('td')];
      cells.forEach((c) => (c.textContent = '–'));
      tr.append(th, ...cells);
      this.el.traits.append(tr);
      this.traitCells.push(cells);
    }

    this.chart = new PopulationChart(this.el.chart as HTMLCanvasElement);
  }

  setFps(fps: number): void {
    this.el.fps.textContent = fps.toFixed(0);
  }

  setStats(s: Stats, speed: number, paused: boolean): void {
    this.el.herb.textContent = fmtInt.format(s.herbivore.count);
    this.el.carn.textContent = fmtInt.format(s.carnivore.count);
    this.el.plant.textContent = fmtInt.format(Math.round(s.plantTotal));

    TRAIT_ROWS.forEach(([, get, digits], r) => {
      [s.herbivore, s.carnivore].forEach((m, c) => {
        const value = get(m);
        this.traitCells[r][c].textContent = m.count === 0 ? '絶滅' : Number.isFinite(value) ? value.toFixed(digits) : '–';
      });
    });

    this.el.tps.textContent = s.ticksPerSecond.toFixed(1);
    this.el.ms.textContent = s.msPerTick > 0 ? s.msPerTick.toFixed(2) : '–';
    if (s.msPerTick > 0) {
      const maxTps = 1000 / s.msPerTick;
      this.el.budget.textContent = `${fmtInt.format(Math.round(maxTps))} tick/s まで（${(maxTps / TICK_HZ).toFixed(1)}×）`;
    }
    const lagging = !paused && speed !== Infinity && s.ticksPerSecond < TICK_HZ * speed * 0.9;
    this.el.tps.classList.toggle('warn', lagging);

    if (!paused) this.chart.push(s.herbivore.count, s.carnivore.count);
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

  setColorMode(mode: ColorMode): void {
    for (const b of this.modeButtons) b.classList.toggle('active', b.dataset.value === mode);
    this.el.lineageLegend.hidden = mode !== 'lineage';
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

function button(label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = label;
  b.addEventListener('click', onClick);
  return b;
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

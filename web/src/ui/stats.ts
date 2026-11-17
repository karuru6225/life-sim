// 統計のウインドウ。地図の上に浮かぶパネルで、長期の記録を 3 つのタブで見せる。
//
// - 大きさ: 大人の大きさの分布の推移（横が時間、縦が大きさの対数、濃さがその時点の割合）と、ある時点の分布
// - 個体数と形質: 種類ごとの個体数と形質の平均の推移
// - 死因と出生: 種類ごとの、死因ごとの死亡数（積み上げ）と生まれた数（線）。1,000 tick あたり
//
// 記録は Worker が HISTORY_EVERY tick ごとに送ってくる。長くなったら隣どうしをまとめて、数を MAX_RECORDS 以下に保つ。
// グラフにマウスを乗せると、その時点の値を表示する（どのタブでも同じ時点を指す）。

import { COLORS } from '../config';
import { DEATH, type HistoryKind, type HistorySample } from '../protocol';

/** 記録の数の上限。超えたら隣どうしをまとめて半分にする */
const MAX_RECORDS = 600;
/** 描き直しの最短間隔（ms）。最大速度では記録が毎秒何度も届くので */
const REDRAW_INTERVAL_MS = 250;
const GEOMETRY_KEY = 'life-sim.statsWindow';

type Tab = 'size' | 'traits' | 'deaths';
type KindKey = 'herb' | 'carn';

const KINDS: [KindKey, string, string][] = [
  ['herb', '草食', COLORS.herbivore],
  ['carn', '肉食', COLORS.carnivore],
];

/**
 * 死因の色。色の役割は「区別」なので、色覚の違いでも隣どうしが見分けられる順に並べた 7 色
 * （パネルの地色 #0c1215 に対してどれも 3:1 以上。dataviz の検証スクリプトで確認済み）。
 * 積み上げの順（下から）もこの順
 */
const CAUSES: [number, string, string][] = [
  [DEATH.STARVED, '餓死', '#3987e5'],
  [DEATH.OLD_AGE, '寿命', '#d95926'],
  [DEATH.EATEN, '捕食', '#199e70'],
  [DEATH.INJURED, 'けが', '#c98500'],
  [DEATH.SICK, '病気', '#d55181'],
  [DEATH.FOUGHT, '敵との争い', '#008300'],
  [DEATH.KIN_FOUGHT, '仲間との争い', '#9085e9'],
];
/** 出生の線の色（文字の色と同じ。死因の色と混ざらないように無彩色にする） */
const BIRTH_COLOR = COLORS.marker;

/** 個体数と形質のタブの行: [キー, 名前, 小数の桁数, 単位] */
const TRAIT_ROWS: [keyof HistoryKind, string, number, string][] = [
  ['count', '個体数', 0, '体'],
  ['size', '大人の大きさ', 2, ''],
  ['speed', '最高速度', 2, ''],
  ['vision', '視野の距離', 1, ''],
  ['fov', '視野角', 0, '°'],
  ['tempPref', '好みの気温', 1, '度'],
  ['invest', '子への投資', 2, ''],
  ['litter', '一度に産む数', 2, ''],
  ['generation', '世代', 0, ''],
  ['brain', '脳の中間の数', 1, ''],
];

// 地のグリッドと軸の線（装飾ではなく目盛りなので控えめに）
const GRID = '#1e2a2e';
const AXIS = '#56696f';
const MUTED = '#93a3a2';
const TEXT = '#dde6e4';
const PANEL = '#0c1215';
const FONT = "11px ui-monospace, 'Cascadia Mono', Consolas, monospace";

/** 2 つの記録をまとめる（形質は区間の長さで重みをつけた平均、出生と死亡は合計） */
function merge(a: HistorySample, b: HistorySample): HistorySample {
  const span = a.span + b.span;
  const wa = a.span / span;
  const wb = b.span / span;
  const kind = (x: HistoryKind, y: HistoryKind): HistoryKind => ({
    count: x.count * wa + y.count * wb,
    size: x.size * wa + y.size * wb,
    speed: x.speed * wa + y.speed * wb,
    vision: x.vision * wa + y.vision * wb,
    fov: x.fov * wa + y.fov * wb,
    tempPref: x.tempPref * wa + y.tempPref * wb,
    invest: x.invest * wa + y.invest * wb,
    litter: x.litter * wa + y.litter * wb,
    generation: x.generation * wa + y.generation * wb,
    brain: x.brain * wa + y.brain * wb,
    births: x.births + y.births,
    deaths: x.deaths.map((d, i) => d + (y.deaths[i] ?? 0)),
    sizeHist: x.sizeHist.map((n, i) => n * wa + (y.sizeHist[i] ?? 0) * wb),
  });
  return { tick: b.tick, span, sizeRange: b.sizeRange, herb: kind(a.herb, b.herb), carn: kind(a.carn, b.carn) };
}

/** 長期の記録。数が MAX_RECORDS を超えたら隣どうしをまとめ、以後は 2 倍の数の標本を 1 つにまとめて足す */
class History {
  private records: HistorySample[] = [];
  private stride = 1;
  private pending: HistorySample | null = null;
  private pendingCount = 0;

  add(s: HistorySample): void {
    this.pending = this.pending ? merge(this.pending, s) : s;
    this.pendingCount++;
    if (this.pendingCount < this.stride) return;
    this.records.push(this.pending);
    this.pending = null;
    this.pendingCount = 0;
    if (this.records.length > MAX_RECORDS) {
      const halved: HistorySample[] = [];
      for (let i = 0; i + 1 < this.records.length; i += 2) halved.push(merge(this.records[i], this.records[i + 1]));
      if (this.records.length % 2 === 1) halved.push(this.records[this.records.length - 1]);
      this.records = halved;
      this.stride *= 2;
    }
  }

  clear(): void {
    this.records = [];
    this.stride = 1;
    this.pending = null;
    this.pendingCount = 0;
  }

  /** 表示に使う記録（まとめ途中のものも最後に含める） */
  all(): HistorySample[] {
    return this.pending ? [...this.records, this.pending] : this.records;
  }
}

/** canvas を表示上の大きさ × 画素比で作り直し、表示上の座標で描ける文脈を返す */
function prepare(canvas: HTMLCanvasElement): { ctx: CanvasRenderingContext2D; w: number; h: number } {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  ctx.font = FONT;
  return { ctx, w, h };
}

/** 目盛りに使うきりのよい間隔（1, 2, 5 × 10 の累乗）のうち、range を count 個以下に分けるもの */
function niceStep(range: number, count: number): number {
  const raw = range / Math.max(1, count);
  const p = 10 ** Math.floor(Math.log10(raw));
  for (const m of [1, 2, 5, 10]) if (m * p >= raw) return m * p;
  return 10 * p;
}

/** tick の短い表記（12k, 1.2M） */
function tickLabel(t: number): string {
  if (t >= 1e6) return `${+(t / 1e6).toFixed(2)}M`;
  if (t >= 1e3) return `${+(t / 1e3).toFixed(1)}k`;
  return String(Math.round(t));
}

/** 率の表示。大きい値に小数は要らないので、桁に応じて小数の桁数を変える */
function fmtRate(v: number): string {
  return fmt(v, Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2);
}

function fmt(v: number, digits: number): string {
  if (!Number.isFinite(v)) return '–';
  return v.toLocaleString('ja-JP', { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** 16 進の色を [r, g, b] に */
function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** 時間の横軸。plot の左端 x0 から幅 pw に、記録の区間 [最初の記録の始まり, 最後の記録の tick] を割り当てる */
interface TimeAxis {
  x0: number;
  pw: number;
  start: number;
  end: number;
}

function timeX(ax: TimeAxis, tick: number): number {
  return ax.x0 + ((tick - ax.start) / Math.max(1, ax.end - ax.start)) * ax.pw;
}

/** x 座標に最も近い記録の番号 */
function indexAt(records: HistorySample[], ax: TimeAxis, x: number): number | null {
  if (records.length === 0 || x < ax.x0 || x > ax.x0 + ax.pw) return null;
  const tick = ax.start + ((x - ax.x0) / ax.pw) * (ax.end - ax.start);
  // 記録 i は区間 (tick − span, tick] を表す
  let lo = 0;
  let hi = records.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (records[mid].tick < tick) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function drawTimeTicks(ctx: CanvasRenderingContext2D, ax: TimeAxis, y: number): void {
  const step = niceStep(ax.end - ax.start, Math.max(2, Math.floor(ax.pw / 70)));
  ctx.fillStyle = MUTED;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'top';
  for (let t = Math.ceil(ax.start / step) * step; t <= ax.end; t += step) {
    const x = timeX(ax, t);
    ctx.fillStyle = AXIS;
    ctx.fillRect(Math.round(x), y, 1, 4);
    // 枠の外にはみ出す数値は描かない（切れた数値は別の値に見える）
    const label = tickLabel(t);
    const half = ctx.measureText(label).width / 2;
    if (x - half < 0 || x + half > ax.x0 + ax.pw + 4) continue;
    ctx.fillStyle = MUTED;
    ctx.fillText(label, x, y + 5);
  }
}

function drawHoverLine(ctx: CanvasRenderingContext2D, x: number, top: number, bottom: number): void {
  ctx.fillStyle = TEXT;
  ctx.globalAlpha = 0.6;
  ctx.fillRect(Math.round(x), top, 1, bottom - top);
  ctx.globalAlpha = 1;
}

export class StatsWindow {
  private readonly root: HTMLElement;
  private readonly el: Record<string, HTMLElement> = {};
  private readonly history = new History();
  private tab: Tab = 'size';
  private sizeKind: KindKey = 'herb';
  /** マウスを乗せている記録の番号（null なら最新） */
  private hover: number | null = null;
  private lastDraw = 0;
  private drawQueued = false;
  /** 時間軸を持つグラフごとの軸（マウスの位置から記録を探すのに使う） */
  private readonly axes = new Map<HTMLCanvasElement, TimeAxis>();
  private readonly traitCanvases: Record<KindKey, HTMLCanvasElement[]> = { herb: [], carn: [] };
  private readonly traitValues: Record<KindKey, HTMLElement[]> = { herb: [], carn: [] };

  constructor(
    root: HTMLElement,
    /** 開いた・閉じたとき */
    private readonly onOpenChange: (open: boolean) => void,
  ) {
    this.root = root;
    root.className = 'stats-win';
    root.hidden = true;
    root.setAttribute('role', 'dialog');
    root.setAttribute('aria-label', '統計');
    root.innerHTML = `
      <header class="stats-head" data-k="head">
        <span class="stats-title">統計</span>
        <div class="stats-tabs" role="tablist">
          <button type="button" role="tab" data-tab="size">大きさ</button>
          <button type="button" role="tab" data-tab="traits">個体数と形質</button>
          <button type="button" role="tab" data-tab="deaths">死因と出生</button>
        </div>
        <button type="button" class="stats-close" data-k="close" aria-label="統計を閉じる" title="閉じる">×</button>
      </header>
      <div class="stats-when" data-k="when">–</div>
      <div class="stats-pane" data-pane="size">
        <div class="stats-row">
          <div class="seg" data-k="sizeKinds"></div>
          <div class="size-summary" data-k="sizeSummary">–</div>
        </div>
        <div class="size-plot">
          <canvas class="size-heat" data-k="heat" aria-label="大人の大きさの分布の推移"></canvas>
          <canvas class="size-hist" data-k="hist" aria-label="ある時点の大人の大きさの分布"></canvas>
        </div>
        <p class="stats-note">縦は大人の大きさ（対数の目盛り）。濃さは、その時点の個体のうちその大きさにいる割合（時点ごとに最も多い段階を最も濃くする）。右はその時点の分布</p>
      </div>
      <div class="stats-pane" data-pane="traits">
        <div class="trait-grid" data-k="traitGrid">
          <span></span>
          <span class="trait-kind"><i class="dot" style="background:${COLORS.herbivore}"></i>草食</span>
          <span class="trait-kind"><i class="tri" style="background:${COLORS.carnivore}"></i>肉食</span>
        </div>
        <p class="stats-note">縦軸はグラフごとに、記録の最小から最大まで</p>
      </div>
      <div class="stats-pane" data-pane="deaths">
        <div data-k="deathBlocks"></div>
        <p class="stats-note">1,000 tick あたりの数。面は死因ごとの死亡数の積み上げ（上端が死亡の合計）、線は生まれた数</p>
      </div>
      <div class="stats-tip" data-k="tip" hidden></div>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) this.el[node.dataset.k!] = node;

    this.el.close.addEventListener('click', () => this.setOpen(false));
    for (const b of root.querySelectorAll<HTMLButtonElement>('[data-tab]')) {
      b.addEventListener('click', () => this.setTab(b.dataset.tab as Tab));
    }
    for (const [key, label] of KINDS) {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.dataset.value = key;
      b.addEventListener('click', () => {
        this.sizeKind = key;
        this.syncButtons();
        this.draw();
      });
      this.el.sizeKinds.append(b);
    }

    // 個体数と形質: 行ごとに [名前, 草食, 肉食]
    for (const [, label, , unit] of TRAIT_ROWS) {
      const name = document.createElement('span');
      name.className = 'trait-name';
      name.textContent = unit && unit !== '体' ? `${label}（${unit}）` : label;
      this.el.traitGrid.append(name);
      for (const [key] of KINDS) {
        const cell = document.createElement('div');
        cell.className = 'trait-cell';
        const value = document.createElement('span');
        value.className = 'trait-value';
        const canvas = document.createElement('canvas');
        canvas.className = 'trait-spark';
        cell.append(value, canvas);
        this.el.traitGrid.append(cell);
        this.traitValues[key].push(value);
        this.traitCanvases[key].push(canvas);
        this.hoverable(canvas);
      }
    }

    // 死因と出生: 種類ごとに [見出し, グラフ, 凡例]
    for (const [key, label, color] of KINDS) {
      const block = document.createElement('div');
      block.className = 'death-block';
      block.innerHTML = `
        <div class="death-title"><i class="${key === 'herb' ? 'dot' : 'tri'}" style="background:${color}"></i>${label}</div>
        <canvas class="death-chart" data-k="death_${key}" aria-label="${label}の死因ごとの死亡数と生まれた数の推移"></canvas>
        <ul class="death-legend" data-k="legend_${key}"></ul>
      `;
      this.el.deathBlocks.append(block);
      for (const node of block.querySelectorAll<HTMLElement>('[data-k]')) this.el[node.dataset.k!] = node;
      this.hoverable(this.el[`death_${key}`] as HTMLCanvasElement);
    }

    const heat = this.el.heat as HTMLCanvasElement;
    this.hoverable(heat);
    heat.addEventListener('mousemove', (e) => this.showHeatTip(e));
    heat.addEventListener('mouseleave', () => (this.el.tip.hidden = true));

    this.makeDraggable();
    this.restoreGeometry();
    new ResizeObserver(() => {
      this.saveGeometry();
      this.draw();
    }).observe(root);
    this.syncButtons();
  }

  get isOpen(): boolean {
    return !this.root.hidden;
  }

  setOpen(open: boolean): void {
    this.root.hidden = !open;
    if (open) this.draw();
    this.onOpenChange(open);
  }

  toggle(): void {
    this.setOpen(!this.isOpen);
  }

  add(sample: HistorySample): void {
    this.history.add(sample);
    this.requestDraw();
  }

  /** 世界を作り直したとき（リセット・読み込み） */
  clear(): void {
    this.history.clear();
    this.hover = null;
    this.requestDraw();
  }

  private setTab(tab: Tab): void {
    this.tab = tab;
    this.hover = null;
    this.el.tip.hidden = true;
    this.syncButtons();
    this.draw();
  }

  private syncButtons(): void {
    for (const b of this.root.querySelectorAll<HTMLButtonElement>('[data-tab]')) {
      const on = b.dataset.tab === this.tab;
      b.setAttribute('aria-selected', String(on));
      b.classList.toggle('active', on);
    }
    for (const p of this.root.querySelectorAll<HTMLElement>('[data-pane]')) p.hidden = p.dataset.pane !== this.tab;
    for (const b of this.el.sizeKinds.querySelectorAll<HTMLButtonElement>('button')) {
      b.classList.toggle('active', b.dataset.value === this.sizeKind);
      b.setAttribute('aria-pressed', String(b.dataset.value === this.sizeKind));
    }
  }

  /** 時間軸のあるグラフにマウスを乗せたら、その時点を全グラフで指す */
  private hoverable(canvas: HTMLCanvasElement): void {
    canvas.addEventListener('mousemove', (e) => {
      const ax = this.axes.get(canvas);
      if (!ax) return;
      const rect = canvas.getBoundingClientRect();
      const i = indexAt(this.history.all(), ax, e.clientX - rect.left);
      if (i !== this.hover) {
        this.hover = i;
        this.draw();
      }
    });
    canvas.addEventListener('mouseleave', () => {
      this.hover = null;
      this.draw();
    });
  }

  private requestDraw(): void {
    if (!this.isOpen || this.drawQueued) return;
    const wait = Math.max(0, REDRAW_INTERVAL_MS - (performance.now() - this.lastDraw));
    this.drawQueued = true;
    setTimeout(() => {
      this.drawQueued = false;
      this.draw();
    }, wait);
  }

  private draw(): void {
    if (!this.isOpen) return;
    this.lastDraw = performance.now();
    const records = this.history.all();
    const at = records.length === 0 ? null : (this.hover ?? records.length - 1);
    this.el.when.textContent =
      at === null
        ? '記録を待っています（200 tick ごとに記録する。リセット・読み込みのたびに最初から）'
        : `${this.hover === null ? '最新' : 'マウスの位置'}: tick ${records[at].tick.toLocaleString('ja-JP')}` +
          `  ·  記録 ${tickLabel(records[0].tick - records[0].span)}〜${tickLabel(records[records.length - 1].tick)}`;
    if (this.tab === 'size') this.drawSize(records, at);
    else if (this.tab === 'traits') this.drawTraits(records, at);
    else this.drawDeaths(records, at);
  }

  private timeAxis(records: HistorySample[], x0: number, pw: number): TimeAxis {
    return { x0, pw, start: records[0].tick - records[0].span, end: records[records.length - 1].tick };
  }

  // ---- 大きさ ----

  /** 表示する大きさの段階の範囲（記録の中で一度でも個体がいた段階の前後 1 段階。最低 8 段階） */
  private sizeBinRange(records: HistorySample[], key: KindKey, bins: number): [number, number] {
    let lo = bins;
    let hi = -1;
    for (const r of records) {
      const h = r[key].sizeHist;
      for (let b = 0; b < h.length; b++) {
        if (h[b] > 0) {
          if (b < lo) lo = b;
          if (b > hi) hi = b;
        }
      }
    }
    if (hi < 0) return [0, bins - 1];
    lo = Math.max(0, lo - 1);
    hi = Math.min(bins - 1, hi + 1);
    while (hi - lo + 1 < 8) {
      if (lo > 0) lo--;
      if (hi - lo + 1 < 8 && hi < bins - 1) hi++;
      if (lo === 0 && hi === bins - 1) break;
    }
    return [lo, hi];
  }

  private drawSize(records: HistorySample[], at: number | null): void {
    const heat = this.el.heat as HTMLCanvasElement;
    const histCanvas = this.el.hist as HTMLCanvasElement;
    const { ctx, w, h } = prepare(heat);
    const side = prepare(histCanvas);
    const key = this.sizeKind;
    const color = key === 'herb' ? COLORS.herbivore : COLORS.carnivore;
    if (at === null) {
      this.el.sizeSummary.textContent = '–';
      this.axes.delete(heat);
      return;
    }
    const bins = records[0][key].sizeHist.length;
    const [lo, hi] = records[0].sizeRange;
    const [b0, b1] = this.sizeBinRange(records, key, bins);
    const shown = b1 - b0 + 1;
    // 縦軸の数値（最も長いのは "0.15"）が切れない幅
    const left = Math.ceil(ctx.measureText('0.15').width) + 10;
    const bottom = 18;
    const top = 4;
    const ph = h - top - bottom;
    const ax = this.timeAxis(records, left, w - left - 4);
    this.axes.set(heat, ax);
    // 段階 b の下端の大きさ
    const edge = (b: number) => lo * (hi / lo) ** (b / bins);
    // 大きさ s の縦の位置（大きいほど上）
    const sizeY = (s: number) => top + ph - ((Math.log(s / lo) / Math.log(hi / lo)) * bins - b0) * (ph / shown);

    // 濃さの図: 1 時点を 1 列、1 段階を 1 行とした小さな画像を作り、拡大して描く
    const img = new ImageData(records.length, shown);
    const [br, bg, bb] = rgb(PANEL);
    const [cr, cg, cb] = rgb(color);
    records.forEach((r, x) => {
      const hist = r[key].sizeHist;
      let max = 0;
      for (let b = b0; b <= b1; b++) max = Math.max(max, hist[b]);
      for (let b = b0; b <= b1; b++) {
        const v = max > 0 ? (hist[b] / max) ** 0.6 : 0;
        const o = ((b1 - b) * records.length + x) * 4;
        img.data[o] = br + (cr - br) * v;
        img.data[o + 1] = bg + (cg - bg) * v;
        img.data[o + 2] = bb + (cb - bb) * v;
        img.data[o + 3] = 255;
      }
    });
    const off = new OffscreenCanvas(records.length, shown);
    off.getContext('2d')!.putImageData(img, 0, 0);
    ctx.imageSmoothingEnabled = false;
    const x0 = timeX(ax, ax.start);
    ctx.drawImage(off, x0, top, timeX(ax, ax.end) - x0, ph);

    // 縦の目盛り（大きさ）
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    for (const s of [0.1, 0.15, 0.2, 0.3, 0.5, 0.7, 1, 1.5, 2, 3, 4]) {
      const y = sizeY(s);
      if (y < top - 0.5 || y > top + ph + 0.5) continue;
      ctx.fillStyle = AXIS;
      ctx.fillRect(left - 4, Math.round(y), 4, 1);
      ctx.fillStyle = MUTED;
      ctx.fillText(String(s), left - 6, y);
    }
    ctx.fillStyle = AXIS;
    ctx.fillRect(left, top + ph, w - left - 4, 1);
    drawTimeTicks(ctx, ax, top + ph + 1);
    if (this.hover !== null) {
      const r = records[this.hover];
      const xa = timeX(ax, r.tick - r.span);
      const xb = timeX(ax, r.tick);
      ctx.strokeStyle = TEXT;
      ctx.lineWidth = 1;
      ctx.strokeRect(Math.round(xa) - 0.5, top - 0.5, Math.max(2, xb - xa) + 1, ph + 1);
    }

    // 右: その時点の分布（横向きの棒。縦の位置は左と同じ）
    const r = records[at];
    const hist = r[key].sizeHist;
    const total = hist.reduce((a, b) => a + b, 0);
    let peak = 0;
    for (let b = b0; b <= b1; b++) peak = Math.max(peak, hist[b]);
    const rowH = ph / shown;
    side.ctx.fillStyle = color;
    for (let b = b0; b <= b1; b++) {
      if (hist[b] <= 0) continue;
      const len = (hist[b] / peak) * (side.w - 8);
      const y = top + (b1 - b) * rowH;
      side.ctx.fillRect(0, y + 1, Math.max(1, len), Math.max(1, rowH - 2));
    }
    side.ctx.fillStyle = AXIS;
    side.ctx.fillRect(0, top, 1, ph);
    side.ctx.fillStyle = MUTED;
    side.ctx.textAlign = 'right';
    side.ctx.textBaseline = 'top';
    if (total > 0) side.ctx.fillText(`最多 ${fmt((peak / total) * 100, 0)}%`, side.w - 2, top + ph + 5);

    // 要約: 平均（記録の値）と、分布から求めた分位点（段階の中で一様に分布していると見なす）
    const quantile = (p: number) => {
      let acc = 0;
      for (let b = 0; b < bins; b++) {
        if (acc + hist[b] >= p * total && hist[b] > 0) {
          const f = (p * total - acc) / hist[b];
          return edge(b) * (edge(b + 1) / edge(b)) ** f;
        }
        acc += hist[b];
      }
      return hi;
    };
    this.el.sizeSummary.textContent =
      total > 0
        ? `${fmt(total, 0)} 体 · 平均 ${fmt(r[key].size, 2)} · 中央 ${fmt(quantile(0.5), 2)} · 10〜90% ${fmt(quantile(0.1), 2)}〜${fmt(quantile(0.9), 2)}`
        : 'いない';
  }

  private showHeatTip(e: MouseEvent): void {
    const heat = this.el.heat as HTMLCanvasElement;
    const ax = this.axes.get(heat);
    const records = this.history.all();
    const tip = this.el.tip;
    if (!ax || records.length === 0) return;
    const rect = heat.getBoundingClientRect();
    const i = indexAt(records, ax, e.clientX - rect.left);
    if (i === null) {
      tip.hidden = true;
      return;
    }
    const key = this.sizeKind;
    const bins = records[0][key].sizeHist.length;
    const [lo, hi] = records[0].sizeRange;
    const [b0, b1] = this.sizeBinRange(records, key, bins);
    const top = 4;
    const ph = heat.clientHeight - top - 18;
    const b = b1 - Math.floor(((e.clientY - rect.top - top) / ph) * (b1 - b0 + 1));
    if (b < b0 || b > b1) {
      tip.hidden = true;
      return;
    }
    const hist = records[i][key].sizeHist;
    const total = hist.reduce((a, c) => a + c, 0);
    const edge = (k: number) => lo * (hi / lo) ** (k / bins);
    tip.textContent = `tick ${records[i].tick.toLocaleString('ja-JP')} · 大きさ ${fmt(edge(b), 2)}〜${fmt(edge(b + 1), 2)} · ${
      total > 0 ? fmt((hist[b] / total) * 100, 1) : '0'
    }%（${fmt(hist[b], 0)} 体）`;
    tip.hidden = false;
    const box = this.root.getBoundingClientRect();
    const x = e.clientX - box.left + 12;
    tip.style.left = `${Math.min(x, box.width - tip.offsetWidth - 8)}px`;
    tip.style.top = `${e.clientY - box.top + 14}px`;
  }

  // ---- 個体数と形質 ----

  private drawTraits(records: HistorySample[], at: number | null): void {
    for (const [key, , color] of KINDS) {
      TRAIT_ROWS.forEach(([field, , digits, unit], row) => {
        const canvas = this.traitCanvases[key][row];
        const { ctx, w, h } = prepare(canvas);
        const value = this.traitValues[key][row];
        if (at === null) {
          value.textContent = '–';
          this.axes.delete(canvas);
          return;
        }
        const v = (r: HistorySample) => r[key][field] as number;
        // 個体がいない時点の形質は値がないので描かない
        const has = (r: HistorySample) => field === 'count' || r[key].count > 0;
        const cur = records[at];
        value.textContent = has(cur) ? `${fmt(v(cur), digits)}${unit === '体' ? ' 体' : ''}` : 'いない';
        let min = Infinity;
        let max = -Infinity;
        for (const r of records) {
          if (!has(r)) continue;
          min = Math.min(min, v(r));
          max = Math.max(max, v(r));
        }
        const ax = this.timeAxis(records, 0, w);
        this.axes.set(canvas, ax);
        if (!Number.isFinite(min)) return;
        // 振れ幅が値の 2% 未満なら、それ以上は拡大しない（ほぼ一定の値の揺れが山脈に見えないように）
        const minSpan = Math.max(Math.abs(max), Math.abs(min)) * 0.02 || 1e-6;
        if (max - min < minSpan) {
          const mid = (max + min) / 2;
          min = mid - minSpan / 2;
          max = mid + minSpan / 2;
        }
        const pad = 3;
        const y = (val: number) => pad + (h - 2 * pad) * (1 - (val - min) / (max - min));
        ctx.fillStyle = GRID;
        ctx.fillRect(0, h - 1, w, 1);
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.5;
        ctx.lineJoin = 'round';
        ctx.beginPath();
        let pen = false;
        for (const r of records) {
          if (!has(r)) {
            pen = false;
            continue;
          }
          const x = timeX(ax, r.tick);
          if (pen) ctx.lineTo(x, y(v(r)));
          else ctx.moveTo(x, y(v(r)));
          pen = true;
        }
        ctx.stroke();
        if (this.hover !== null) drawHoverLine(ctx, timeX(ax, records[this.hover].tick), 0, h);
        canvas.title = `最小 ${fmt(min, digits)} / 最大 ${fmt(max, digits)}`;
      });
    }
  }

  // ---- 死因と出生 ----

  private drawDeaths(records: HistorySample[], at: number | null): void {
    for (const [key] of KINDS) {
      const canvas = this.el[`death_${key}`] as HTMLCanvasElement;
      const legend = this.el[`legend_${key}`];
      const { ctx, w, h } = prepare(canvas);
      if (at === null) {
        legend.innerHTML = '';
        this.axes.delete(canvas);
        continue;
      }
      const per = (n: number, r: HistorySample) => (n / Math.max(1, r.span)) * 1000;
      // 積み上げの各段の上端
      const stacks = records.map((r) => {
        let acc = 0;
        return CAUSES.map(([cause]) => (acc += per(r[key].deaths[cause] ?? 0, r)));
      });
      let max = 0;
      records.forEach((r, i) => (max = Math.max(max, stacks[i][CAUSES.length - 1], per(r[key].births, r))));
      const step = niceStep(max || 1, 3);
      const yMax = Math.max(step, Math.ceil(max / step) * step);
      // 縦軸の数値が切れないよう、左の余白は最も長い数値の幅から決める
      const labelDigits = step < 1 ? 1 : 0;
      const left = Math.ceil(ctx.measureText(fmt(yMax, labelDigits)).width) + 10;
      const top = 6;
      const bottom = 18;
      const ph = h - top - bottom;
      const ax = this.timeAxis(records, left, w - left - 4);
      this.axes.set(canvas, ax);
      const y = (v: number) => top + ph * (1 - v / yMax);

      // 目盛りの線と数値
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      for (let v = 0; v <= yMax + 1e-9; v += step) {
        ctx.fillStyle = v === 0 ? AXIS : GRID;
        ctx.fillRect(left, Math.round(y(v)), w - left - 4, 1);
        ctx.fillStyle = MUTED;
        ctx.fillText(fmt(v, labelDigits), left - 6, y(v));
      }
      drawTimeTicks(ctx, ax, top + ph + 1);

      // 死因ごとの面（下から順に。各記録は区間 [tick − span, tick] を表すので、区間の両端で同じ高さの段にする）
      CAUSES.forEach(([, , color], c) => {
        ctx.fillStyle = color;
        ctx.beginPath();
        records.forEach((r, i) => {
          const xa = timeX(ax, r.tick - r.span);
          const xb = timeX(ax, r.tick);
          if (i === 0) ctx.moveTo(xa, y(stacks[i][c]));
          else ctx.lineTo(xa, y(stacks[i][c]));
          ctx.lineTo(xb, y(stacks[i][c]));
        });
        for (let i = records.length - 1; i >= 0; i--) {
          const r = records[i];
          const below = c > 0 ? stacks[i][c - 1] : 0;
          ctx.lineTo(timeX(ax, r.tick), y(below));
          ctx.lineTo(timeX(ax, r.tick - r.span), y(below));
        }
        ctx.closePath();
        ctx.fill();
      });

      // 生まれた数の線（面の上に、地色の縁取りを付けて重ねる）
      const birthPath = () => {
        ctx.beginPath();
        records.forEach((r, i) => {
          const xa = timeX(ax, r.tick - r.span);
          const xb = timeX(ax, r.tick);
          const v = y(per(r[key].births, r));
          if (i === 0) ctx.moveTo(xa, v);
          else ctx.lineTo(xa, v);
          ctx.lineTo(xb, v);
        });
      };
      ctx.lineJoin = 'round';
      birthPath();
      ctx.strokeStyle = PANEL;
      ctx.lineWidth = 4;
      ctx.stroke();
      birthPath();
      ctx.strokeStyle = BIRTH_COLOR;
      ctx.lineWidth = 2;
      ctx.stroke();
      if (this.hover !== null) {
        const r = records[this.hover];
        drawHoverLine(ctx, timeX(ax, r.tick - r.span / 2), top, top + ph);
      }

      // 凡例（その時点の値つき）。上の段ほど上に並べ、グラフの積み上げの順と合わせる
      const r = records[at];
      const total = stacks[at][CAUSES.length - 1];
      const items = [
        `<li class="birth"><i class="swatch-line" style="background:${BIRTH_COLOR}"></i>出生<b>${fmtRate(per(r[key].births, r))}</b></li>`,
        `<li class="total"><i></i>死亡の合計<b>${fmtRate(total)}</b></li>`,
        ...[...CAUSES]
          .reverse()
          .map(([cause, label, color]) => `<li><i class="swatch" style="background:${color}"></i>${label}<b>${fmtRate(per(r[key].deaths[cause] ?? 0, r))}</b></li>`),
      ];
      legend.innerHTML = items.join('');
    }
  }

  // ---- ウインドウの移動と位置の記憶 ----

  private makeDraggable(): void {
    const head = this.el.head;
    head.addEventListener('pointerdown', (e) => {
      if ((e.target as HTMLElement).closest('button')) return;
      const rect = this.root.getBoundingClientRect();
      const dx = e.clientX - rect.left;
      const dy = e.clientY - rect.top;
      head.setPointerCapture(e.pointerId);
      const move = (ev: PointerEvent) => {
        const x = Math.min(Math.max(0, ev.clientX - dx), window.innerWidth - 80);
        const y = Math.min(Math.max(0, ev.clientY - dy), window.innerHeight - 40);
        this.root.style.left = `${x}px`;
        this.root.style.top = `${y}px`;
      };
      const up = () => {
        head.removeEventListener('pointermove', move);
        head.removeEventListener('pointerup', up);
        this.saveGeometry();
      };
      head.addEventListener('pointermove', move);
      head.addEventListener('pointerup', up);
    });
  }

  private saveGeometry(): void {
    if (this.root.hidden) return;
    const r = this.root.getBoundingClientRect();
    try {
      localStorage.setItem(GEOMETRY_KEY, JSON.stringify({ left: r.left, top: r.top, width: r.width, height: r.height }));
    } catch {
      // 覚えておけなくても使える
    }
  }

  private restoreGeometry(): void {
    try {
      const g = JSON.parse(localStorage.getItem(GEOMETRY_KEY) ?? 'null') as Record<string, number> | null;
      if (!g) return;
      const width = Math.min(g.width, window.innerWidth - 16);
      const height = Math.min(g.height, window.innerHeight - 16);
      this.root.style.width = `${width}px`;
      this.root.style.height = `${height}px`;
      this.root.style.left = `${Math.min(Math.max(0, g.left), window.innerWidth - width)}px`;
      this.root.style.top = `${Math.min(Math.max(0, g.top), window.innerHeight - height)}px`;
    } catch {
      // 既定の位置のまま
    }
  }
}

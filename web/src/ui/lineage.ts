// 系統色（標識色の遺伝子）のヒストグラムと、選んだ系統の統計パネル。
//
// ヒストグラム: 色相を BINS 段階に分け、段階ごとの個体数を棒で描く。棒はその系統色で塗る。
// 草食と肉食は数が 1 桁違うので、それぞれ自分の最大値を基準に高さを決める。棒をクリックすると、その段階を系統として選ぶ。
//
// 統計パネル: 選んだ系統の個体数の推移と、形質などの平均を種類全体の平均と並べて表示する。

import { COLORS, HISTORY_LENGTH } from '../config';
import { CARNIVORE, DEATH, HERBIVORE, LINEAGE, LINEAGE_DEATH_WINDOW, LINEAGE_LEN, type LineageSelection } from '../protocol';
import { HUE_PALETTE, hueIndex } from '../render/palette';

export const BINS = 36;

export class LineageHistogram {
  private readonly counts = [new Array<number>(BINS).fill(0), new Array<number>(BINS).fill(0)];
  private readonly colors: string[];
  private selected: LineageSelection | null = null;

  constructor(
    private readonly canvases: [HTMLCanvasElement, HTMLCanvasElement],
    private readonly maxLabels: [HTMLElement, HTMLElement],
    /** 棒をクリックしたとき（種類と、その段階の色相の範囲） */
    onSelect: (sel: LineageSelection) => void,
  ) {
    // 段階の中央の色相の色
    this.colors = Array.from({ length: BINS }, (_, b) => HUE_PALETTE[Math.floor(((b + 0.5) / BINS) * HUE_PALETTE.length)]);
    canvases.forEach((canvas, k) => {
      canvas.style.cursor = 'pointer';
      canvas.addEventListener('click', (e) => {
        const rect = canvas.getBoundingClientRect();
        const bin = Math.min(BINS - 1, Math.max(0, Math.floor(((e.clientX - rect.left) / rect.width) * BINS)));
        onSelect({ kind: k === 0 ? HERBIVORE : CARNIVORE, from: bin / BINS, width: 1 / BINS });
      });
    });
  }

  /** 選んだ系統を、該当する棒の下の印で示す（null で消す） */
  setSelected(sel: LineageSelection | null): void {
    this.selected = sel;
    for (const k of [0, 1]) this.draw(k);
  }

  /** data: [x, y, 種類, 大きさ, 標識色, 向き] × count */
  update(data: Float32Array, count: number, stride: number): void {
    for (const c of this.counts) c.fill(0);
    for (let i = 0; i < count; i++) {
      const o = i * stride;
      const kind = data[o + 2] === HERBIVORE ? 0 : data[o + 2] === CARNIVORE ? 1 : -1;
      if (kind < 0) continue;
      const bin = Math.min(BINS - 1, Math.floor(data[o + 4] * BINS));
      this.counts[kind][bin]++;
    }
    for (const k of [0, 1]) this.draw(k);
  }

  private draw(k: number): void {
    const canvas = this.canvases[k];
    const counts = this.counts[k];
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const max = Math.max(1, ...counts);
    this.maxLabels[k].textContent = `最大 ${max.toLocaleString('ja-JP')} 体`;
    const barW = w / BINS;
    // 下の 4px は選択の印に使う
    const chartH = h - 4;
    for (let b = 0; b < BINS; b++) {
      const bh = (counts[b] / max) * (chartH - 1);
      if (counts[b] > 0) {
        ctx.fillStyle = this.colors[b];
        // 1 体でも見えるよう、高さは最低 1px
        ctx.fillRect(b * barW + 0.5, chartH - Math.max(1, bh), barW - 1, Math.max(1, bh));
      }
    }
    // 底の線
    ctx.fillStyle = COLORS.marker;
    ctx.globalAlpha = 0.3;
    ctx.fillRect(0, chartH - 1, w, 1);
    ctx.globalAlpha = 1;
    // 選んだ系統の範囲の下に印
    const sel = this.selected;
    if (sel && sel.kind === (k === 0 ? HERBIVORE : CARNIVORE)) {
      ctx.fillStyle = COLORS.marker;
      const x0 = sel.from * w;
      const x1 = (sel.from + sel.width) * w;
      ctx.fillRect(x0, h - 3, Math.min(w, x1) - x0, 3);
      // 1 をまたぐ範囲は左端にも
      if (x1 > w) ctx.fillRect(0, h - 3, x1 - w, 3);
    }
  }
}

/** 統計パネルの行: [見出し, 値の位置, 小数の桁数, 単位] */
const ROWS: [string, number, number, string][] = [
  ['大きさ（成体時）', LINEAGE.size, 2, ''],
  ['最高速度', LINEAGE.speed, 2, ''],
  ['視野', LINEAGE.vision, 1, 'px'],
  ['視野角', LINEAGE.fovDeg, 0, '°'],
  ['好みの気温', LINEAGE.tempPref, 1, '°'],
  ['子への投資', LINEAGE.invest, 2, ''],
  ['一度に産む数', LINEAGE.litter, 2, ''],
  ['繁殖シグナル', LINEAGE.signal, 2, ''],
  ['脳: 1 層目のニューロン', LINEAGE.brain1, 1, ''],
  ['脳: 2 層目のニューロン', LINEAGE.brain2, 1, ''],
  ['世代', LINEAGE.generation, 0, ''],
  ['蓄え ÷ 繁殖の必要量', LINEAGE.fed, 2, ''],
  ['幼体の割合', LINEAGE.juvenile, 2, ''],
  ['寒い側にいる割合', LINEAGE.coldShare, 2, ''],
  ['重心からの距離の平均', LINEAGE.spread, 0, 'px'],
];

/** 死因の短い名前（DEATH の番号順） */
const DEATH_SHORT: [number, string][] = [
  [DEATH.STARVED, '餓死'],
  [DEATH.OLD_AGE, '寿命'],
  [DEATH.EATEN, '食べられた'],
  [DEATH.INJURED, 'けが'],
  [DEATH.SICK, '病気'],
  [DEATH.FOUGHT, '争い'],
];

export class LineagePanel {
  private readonly el: Record<string, HTMLElement> = {};
  private readonly cells: HTMLElement[][] = [];
  private history: number[] = [];

  constructor(
    private readonly root: HTMLElement,
    onClose: () => void,
    /** 系統の中からランダムに 1 体選ぶ */
    onPick: () => void,
  ) {
    root.innerHTML = `
      <div class="lin-head">
        <i class="swatch" data-k="swatch"></i>
        <span class="lin-title" data-k="title">–</span>
        <button type="button" data-k="pick" title="この系統の個体を 1 体ランダムに選び、カメラで追う">1 体選ぶ</button>
        <button type="button" class="insp-close" data-k="close" aria-label="系統の選択を解除">×</button>
      </div>
      <div class="lin-count"><span class="value" data-k="count">–</span><span class="unit">体</span></div>
      <canvas class="lin-chart" data-k="chart" aria-label="選んだ系統の個体数の推移"></canvas>
      <table class="trait-table lin-table">
        <thead><tr><th></th><th>系統</th><th>種類全体</th></tr></thead>
        <tbody data-k="rows"></tbody>
      </table>
      <table class="trait-table lin-table">
        <caption>直近 ${LINEAGE_DEATH_WINDOW.toLocaleString('ja-JP')} tick の死因（多い順）</caption>
        <thead><tr><th></th><th>系統</th><th>種類全体</th></tr></thead>
        <tbody data-k="deaths"></tbody>
      </table>
      <p class="legend-note">地図では系統の外の個体を薄く描き、重心に十字、重心からの距離の平均を半径とする円を描く</p>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) this.el[node.dataset.k!] = node;
    this.el.close.addEventListener('click', onClose);
    this.el.pick.addEventListener('click', onPick);
    for (const [label] of ROWS) {
      const tr = document.createElement('tr');
      const th = document.createElement('th');
      th.textContent = label;
      tr.append(th);
      const cells = [document.createElement('td'), document.createElement('td')];
      cells.forEach((c) => (c.textContent = '–'));
      tr.append(...cells);
      this.el.rows.append(tr);
      this.cells.push(cells);
    }
    root.hidden = true;
  }

  show(sel: LineageSelection): void {
    this.root.hidden = false;
    this.history = [];
    const center = sel.from + sel.width / 2;
    this.el.swatch.style.background = HUE_PALETTE[hueIndex(((center % 1) + 1) % 1)];
    const deg = (v: number) => Math.round((((v % 1) + 1) % 1) * 360);
    this.el.title.textContent = `${sel.kind === HERBIVORE ? '草食' : '肉食'}の系統 ${deg(sel.from)}°〜${deg(sel.from + sel.width)}°`;
    this.el.count.textContent = '–';
    this.cells.forEach((cells) => cells.forEach((c) => (c.textContent = '–')));
    this.drawChart();
  }

  hide(): void {
    this.root.hidden = true;
  }

  update(stats: number[]): void {
    const n = stats[LINEAGE.count];
    this.el.count.textContent = n.toLocaleString('ja-JP');
    this.history.push(n);
    if (this.history.length > HISTORY_LENGTH) this.history.shift();
    ROWS.forEach(([, at, digits, unit], r) => {
      const [mine, all] = this.cells[r];
      mine.textContent = n === 0 ? '絶滅' : `${stats[at].toFixed(digits)}${unit}`;
      all.textContent = stats[LINEAGE_LEN + LINEAGE.count] === 0 ? '絶滅' : `${stats[LINEAGE_LEN + at].toFixed(digits)}${unit}`;
    });
    this.showDeaths(stats);
    this.drawChart();
  }

  /** 死因を多い順に、系統の内訳と種類全体の内訳を並べて表示する */
  private showDeaths(stats: number[]): void {
    const rows = DEATH_SHORT.map(([cause, label]) => ({
      label,
      mine: stats[LINEAGE.deaths + cause - 1],
      all: stats[LINEAGE_LEN + LINEAGE.deaths + cause - 1],
    }));
    const sum = (k: 'mine' | 'all') => rows.reduce((a, r) => a + r[k], 0);
    const [mineTotal, allTotal] = [sum('mine'), sum('all')];
    const pct = (n: number, total: number) => (total > 0 ? `${Math.round((n / total) * 100)}%` : '–');
    const tbody = this.el.deaths;
    tbody.textContent = '';
    if (mineTotal === 0) {
      const tr = document.createElement('tr');
      tr.innerHTML = '<th colspan="3">死亡なし</th>';
      tbody.append(tr);
      return;
    }
    rows
      .filter((r) => r.mine > 0)
      .sort((a, b) => b.mine - a.mine)
      .forEach((r, k) => {
        const tr = document.createElement('tr');
        const th = document.createElement('th');
        // 最も多い死因は見出しの文字を本文色にして目立たせる
        th.textContent = k === 0 ? `${r.label}（最多）` : r.label;
        if (k === 0) th.classList.add('lin-top');
        const mine = document.createElement('td');
        mine.textContent = `${r.mine.toLocaleString('ja-JP')} 体 ${pct(r.mine, mineTotal)}`;
        const all = document.createElement('td');
        all.textContent = pct(r.all, allTotal);
        tr.append(th, mine, all);
        tbody.append(tr);
      });
  }

  private drawChart(): void {
    const canvas = this.el.chart as HTMLCanvasElement;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const values = this.history;
    if (values.length < 2) return;
    // 縦軸は 0 から最大値まで（系統の消長を誇張しないよう、0 を含める）
    const max = Math.max(1, ...values);
    ctx.strokeStyle = COLORS.marker;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    values.forEach((v, i) => {
      const x = (i / (HISTORY_LENGTH - 1)) * w;
      const y = h - 1 - (v / max) * (h - 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
  }
}

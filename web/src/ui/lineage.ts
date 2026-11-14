// 系統色（標識色の遺伝子）のヒストグラム。色相を BINS 段階に分け、段階ごとの個体数を棒で描く。
// 棒はその系統色で塗る。草食と肉食は数が 1 桁違うので、それぞれ自分の最大値を基準に高さを決める。

import { COLORS } from '../config';
import { CARNIVORE, HERBIVORE } from '../protocol';
import { HUE_PALETTE } from '../render/palette';

const BINS = 36;

export class LineageHistogram {
  private readonly counts = [new Array<number>(BINS).fill(0), new Array<number>(BINS).fill(0)];
  private readonly colors: string[];

  constructor(
    private readonly canvases: [HTMLCanvasElement, HTMLCanvasElement],
    private readonly maxLabels: [HTMLElement, HTMLElement],
  ) {
    // 段階の中央の色相の色
    this.colors = Array.from({ length: BINS }, (_, b) => HUE_PALETTE[Math.floor(((b + 0.5) / BINS) * HUE_PALETTE.length)]);
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
    for (let b = 0; b < BINS; b++) {
      const bh = (counts[b] / max) * (h - 1);
      if (counts[b] > 0) {
        ctx.fillStyle = this.colors[b];
        // 1 体でも見えるよう、高さは最低 1px
        ctx.fillRect(b * barW + 0.5, h - Math.max(1, bh), barW - 1, Math.max(1, bh));
      }
    }
    // 底の線
    ctx.fillStyle = COLORS.marker;
    ctx.globalAlpha = 0.3;
    ctx.fillRect(0, h - 1, w, 1);
    ctx.globalAlpha = 1;
  }
}

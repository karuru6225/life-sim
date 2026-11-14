// 地形の描画。マス目 1 つを 1 ピクセルにした画像を作り、世界の大きさに引き伸ばして植物の下に敷く。
// 表示は 2 通り:
//   植生: 水域は青、陸は肥沃度が高いほど少し明るい
//   気温: 寒い所ほど青紫、暑い所ほど赤い（寒い陸と見分けられるよう、水域は黒に近い色）
// 地形は世界を作り直すまで変わらないので、作り直したときと表示を切り替えたときだけ描く。

import { Color, Sprite, Texture } from 'pixi.js';
import { COLORS } from '../config';

export type MapMode = 'vegetation' | 'temperature';

export class TerrainLayer {
  readonly sprite = new Sprite();
  private canvas = document.createElement('canvas');
  private texture: Texture | null = null;
  private terrain: Float32Array | null = null;
  private temperature: Float32Array | null = null;
  private cols = 0;
  private rows = 0;
  private mode: MapMode = 'vegetation';
  /** 陸の気温の最低・最高（凡例に使う） */
  tempRange: [number, number] = [0, 0];

  constructor(private readonly cellSize: number) {}

  /** terrain: 水は −1、陸は肥沃度 0〜1。temperature: 平均気温（度）。どちらも cols × rows（行優先） */
  setData(terrain: Float32Array, temperature: Float32Array, cols: number, rows: number): void {
    this.terrain = terrain;
    this.temperature = temperature;
    this.cols = cols;
    this.rows = rows;
    let [min, max] = [Infinity, -Infinity];
    for (let i = 0; i < cols * rows; i++) {
      if (terrain[i] < 0) continue;
      min = Math.min(min, temperature[i]);
      max = Math.max(max, temperature[i]);
    }
    this.tempRange = [min, max];
    this.draw();
  }

  setMode(mode: MapMode): void {
    this.mode = mode;
    this.draw();
  }

  private draw(): void {
    const { terrain, temperature, cols, rows } = this;
    if (!terrain || !temperature) return;
    this.canvas.width = cols;
    this.canvas.height = rows;
    const ctx = this.canvas.getContext('2d')!;
    const image = ctx.createImageData(cols, rows);
    const rgb = (c: string) => new Color(c).toUint8RgbArray();
    const water = rgb(this.mode === 'vegetation' ? COLORS.water : COLORS.tempWater);
    const poor = rgb(COLORS.landPoor);
    const rich = rgb(COLORS.landRich);
    const cold = rgb(COLORS.tempCold);
    const mild = rgb(COLORS.tempMild);
    const hot = rgb(COLORS.tempHot);
    const [tmin, tmax] = this.tempRange;
    const mix = (a: Uint8ClampedArray | number[], b: Uint8ClampedArray | number[], t: number, ch: number) =>
      a[ch] + (b[ch] - a[ch]) * t;
    for (let i = 0; i < cols * rows; i++) {
      const o = i * 4;
      for (let ch = 0; ch < 3; ch++) {
        let v: number;
        if (terrain[i] < 0) {
          v = water[ch];
        } else if (this.mode === 'vegetation') {
          v = mix(poor, rich, terrain[i], ch);
        } else {
          // 陸の最低気温〜最高気温を 0〜1 にして、寒色 → 中間 → 暖色
          const t = (temperature[i] - tmin) / Math.max(1e-6, tmax - tmin);
          v = t < 0.5 ? mix(cold, mild, t * 2, ch) : mix(mild, hot, (t - 0.5) * 2, ch);
        }
        image.data[o + ch] = v;
      }
      image.data[o + 3] = 255;
    }
    ctx.putImageData(image, 0, 0);
    this.texture?.destroy(true);
    this.texture = Texture.from(this.canvas);
    this.texture.source.scaleMode = 'linear';
    this.sprite.texture = this.texture;
    this.sprite.width = cols * this.cellSize;
    this.sprite.height = rows * this.cellSize;
  }
}

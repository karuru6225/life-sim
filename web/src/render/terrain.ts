// 地形の描画。マス目 1 つを 1 ピクセルにした画像を作り、世界の大きさに引き伸ばして植物の下に敷く。
// 水域は青、陸は肥沃度が高いほど少し明るく描く。地形は世界を作り直すまで変わらないので、作り直したときだけ描く。

import { Color, Sprite, Texture } from 'pixi.js';
import { COLORS } from '../config';

export class TerrainLayer {
  readonly sprite = new Sprite();
  private canvas = document.createElement('canvas');
  private texture: Texture | null = null;

  constructor(private readonly cellSize: number) {}

  /** terrain: cols × rows（行優先）。水は −1、陸は肥沃度 0〜1 */
  apply(terrain: Float32Array, cols: number, rows: number): void {
    this.canvas.width = cols;
    this.canvas.height = rows;
    const ctx = this.canvas.getContext('2d')!;
    const image = ctx.createImageData(cols, rows);
    const water = new Color(COLORS.water).toUint8RgbArray();
    const poor = new Color(COLORS.landPoor).toUint8RgbArray();
    const rich = new Color(COLORS.landRich).toUint8RgbArray();
    for (let i = 0; i < cols * rows; i++) {
      const v = terrain[i];
      const o = i * 4;
      for (let ch = 0; ch < 3; ch++) {
        image.data[o + ch] = v < 0 ? water[ch] : poor[ch] + (rich[ch] - poor[ch]) * v;
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

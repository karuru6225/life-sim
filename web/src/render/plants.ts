// 植物の描画。マス目 1 つを 1 ピクセルにした画像を作り、世界の大きさに引き伸ばして敷く。
// 引き伸ばすときに補間されるので、マス目の境界がぼけて地面の濃淡のように見える。

import { Color, Sprite, Texture } from 'pixi.js';
import { COLORS } from '../config';

export class PlantLayer {
  readonly sprite: Sprite;
  private canvas = document.createElement('canvas');
  private ctx = this.canvas.getContext('2d')!;
  private image: ImageData | null = null;
  private texture: Texture | null = null;
  private readonly rgb = new Color(COLORS.plant).toUint8RgbArray();

  constructor(
    private readonly cellSize: number,
    private readonly capacity: number,
  ) {
    this.sprite = new Sprite();
  }

  /** biomass: cols × rows（行優先） */
  apply(biomass: Float32Array, cols: number, rows: number): void {
    if (!this.image || this.image.width !== cols || this.image.height !== rows) {
      this.canvas.width = cols;
      this.canvas.height = rows;
      this.image = this.ctx.createImageData(cols, rows);
      this.texture?.destroy(true);
      this.texture = Texture.from(this.canvas);
      this.texture.source.scaleMode = 'linear';
      this.sprite.texture = this.texture;
      this.sprite.width = cols * this.cellSize;
      this.sprite.height = rows * this.cellSize;
    }
    const px = this.image.data;
    const [r, g, b] = this.rgb;
    for (let i = 0; i < cols * rows; i++) {
      const o = i * 4;
      px[o] = r;
      px[o + 1] = g;
      px[o + 2] = b;
      // 植物量に比例した不透明度。満杯で COLORS.plant そのものになる
      px[o + 3] = Math.min(1, biomass[i] / this.capacity) * 255;
    }
    this.ctx.putImageData(this.image, 0, 0);
    this.texture!.source.update();
  }
}

// 植物（と死体）の描画。マス目 1 つを 1 ピクセルにした画像を作り、世界の大きさに引き伸ばして敷く。
// 引き伸ばすときに補間されるので、マス目の境界がぼけて地面の濃淡のように見える。
// 量を不透明度で表す。同じ仕組みで、色と基準の量を変えて死体の層にも使う。

import { Color, Sprite, Texture } from 'pixi.js';
import { COLORS } from '../config';

export class PlantLayer {
  readonly sprite: Sprite;
  private canvas = document.createElement('canvas');
  private ctx = this.canvas.getContext('2d')!;
  private image: ImageData | null = null;
  private texture: Texture | null = null;
  private readonly rgb: Uint8ClampedArray | number[];

  constructor(
    private readonly cellSize: number,
    /** この量で不透明度が最大になる */
    private readonly capacity: number,
    color: string = COLORS.plant,
    /** 量が threshold 以上あるときの最低の不透明度（0〜1）。少量でも見落とさないようにする */
    private readonly minAlpha = 0,
    /** これより少ない量は描かない（minAlpha が 0 のときは使わない） */
    private readonly threshold = 0.05,
  ) {
    this.sprite = new Sprite();
    this.rgb = new Color(color).toUint8RgbArray();
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
      // 量に比例した不透明度。capacity で色そのものになる
      const a = Math.min(1, biomass[i] / this.capacity);
      if (this.minAlpha === 0) px[o + 3] = a * 255;
      else px[o + 3] = biomass[i] >= this.threshold ? (this.minAlpha + (1 - this.minAlpha) * a) * 255 : 0;
    }
    this.ctx.putImageData(this.image, 0, 0);
    this.texture!.source.update();
  }
}

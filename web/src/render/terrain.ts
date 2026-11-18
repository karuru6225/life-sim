// 地形の描画。マス目 1 つを 1 ピクセルにした画像を作り、世界の大きさに引き伸ばして植物の下に敷く。
// 表示は 6 通り:
//   植生: 水域は青、陸は土の肥えかた（植生の種類）が高いほど少し明るい。季節で変わらない
//   平均気温: 寒い所ほど青紫、暑い所ほど赤い（寒い陸と見分けられるよう、水域は黒に近い色）
//   今の気温: 平均気温に季節の変化を加えたもの。色の範囲は 1 年を通して同じにして、季節の移り変わりが色の変化で見えるようにする
//   育ちやすさ: 今の植物の育ちやすさ（土の肥えかた × 今の気温による育ちやすさ）。冬は寒い地域が暗くなる
//   標高: 陸は低い所ほど暗い緑、高い所ほど明るい灰茶。水域は浅い所ほど明るく、深い所ほど暗い青
//   林と草原: 植物の層の割合。草原（地面の草が多い）は黄土色、林（茂みと枝が多い）は深い緑。季節で変わらない
// 植生と平均気温は世界を作り直すまで変わらない。今の気温と育ちやすさは約 1 秒ごとに届く値で描き直す。

import { Color, Sprite, Texture } from 'pixi.js';
import { COLORS, SIM_PARAMS } from '../config';

export type MapMode = 'vegetation' | 'elevation' | 'woodland' | 'temperature' | 'tempNow' | 'growth';

export class TerrainLayer {
  readonly sprite = new Sprite();
  private canvas = document.createElement('canvas');
  private texture: Texture | null = null;
  private terrain: Float32Array | null = null;
  private temperature: Float32Array | null = null;
  private tempNow: Float32Array | null = null;
  private fertility: Float32Array | null = null;
  private wood: Float32Array | null = null;
  private elevation: Float32Array | null = null;
  private cols = 0;
  private rows = 0;
  private mode: MapMode = 'vegetation';
  /** 陸の平均気温の最低・最高（凡例に使う） */
  tempRange: [number, number] = [0, 0];

  constructor(private readonly cellSize: number) {}

  /** 今の気温の色の範囲: 平均気温の範囲を、季節の振れ幅のぶん広げたもの（1 年を通して同じ） */
  get tempNowRange(): [number, number] {
    const amp = SIM_PARAMS.season_temp;
    return [this.tempRange[0] - amp, this.tempRange[1] + amp];
  }

  get currentMode(): MapMode {
    return this.mode;
  }

  /**
   * terrain: 水は −1、陸は土の肥えかた 0〜1。temperature: 平均気温（度）。wood: 林の度合い（陸は 0〜1）。
   * elevation: 標高（陸は 0〜1、水は深さを 0〜−1）。どれも cols × rows（行優先）
   */
  setData(
    terrain: Float32Array,
    temperature: Float32Array,
    wood: Float32Array,
    elevation: Float32Array,
    cols: number,
    rows: number,
  ): void {
    this.elevation = elevation;
    this.terrain = terrain;
    this.temperature = temperature;
    this.wood = wood;
    this.tempNow = null;
    this.fertility = null;
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

  /** 季節で変わるデータ（今の気温と育ちやすさ）。その表示のときだけ描き直す */
  setClimate(tempNow: Float32Array, fertility: Float32Array): void {
    this.tempNow = tempNow;
    this.fertility = fertility;
    if (this.mode === 'tempNow' || this.mode === 'growth') this.draw();
  }

  setMode(mode: MapMode): void {
    this.mode = mode;
    this.draw();
  }

  private draw(): void {
    const { terrain, cols, rows, mode } = this;
    if (!terrain || !this.temperature) return;
    const temperature = mode === 'tempNow' ? this.tempNow : this.temperature;
    if (mode === 'tempNow' && !temperature) return; // まだ届いていない
    if (mode === 'growth' && !this.fertility) return;
    this.canvas.width = cols;
    this.canvas.height = rows;
    const ctx = this.canvas.getContext('2d')!;
    const image = ctx.createImageData(cols, rows);
    const rgb = (c: string) => new Color(c).toUint8RgbArray();
    const temperatureMode = mode === 'temperature' || mode === 'tempNow';
    const water = rgb(temperatureMode ? COLORS.tempWater : COLORS.water);
    const poor = rgb(COLORS.landPoor);
    const rich = rgb(mode === 'growth' ? COLORS.growthHigh : COLORS.landRich);
    const grassland = rgb(COLORS.grassland);
    const woodland = rgb(COLORS.woodland);
    const lowland = rgb(COLORS.lowland);
    const highland = rgb(COLORS.highland);
    const shallow = rgb(COLORS.shallow);
    const deep = rgb(COLORS.deep);
    const elev = this.elevation;
    const cold = rgb(COLORS.tempCold);
    const mild = rgb(COLORS.tempMild);
    const hot = rgb(COLORS.tempHot);
    const [tmin, tmax] = mode === 'tempNow' ? this.tempNowRange : this.tempRange;
    // 育ちやすさは雨の帯の中で 1 を超えるので、今の陸の最大値で割って 0〜1 にする（帯の形が見えるように）
    let growthMax = 1;
    if (mode === 'growth') {
      growthMax = 0;
      for (const f of this.fertility!) growthMax = Math.max(growthMax, f);
      growthMax = Math.max(growthMax, 1e-6);
    }
    const mix = (a: Uint8ClampedArray | number[], b: Uint8ClampedArray | number[], t: number, ch: number) =>
      a[ch] + (b[ch] - a[ch]) * t;
    for (let i = 0; i < cols * rows; i++) {
      const o = i * 4;
      for (let ch = 0; ch < 3; ch++) {
        let v: number;
        if (mode === 'elevation' && elev) {
          v = elev[i] < 0 || terrain[i] < 0 ? mix(shallow, deep, Math.min(1, -elev[i]), ch) : mix(lowland, highland, elev[i], ch);
        } else if (terrain[i] < 0) {
          v = water[ch];
        } else if (mode === 'vegetation') {
          v = mix(poor, rich, terrain[i], ch);
        } else if (mode === 'woodland') {
          v = mix(grassland, woodland, this.wood ? this.wood[i] : 0.5, ch);
        } else if (mode === 'growth') {
          v = mix(poor, rich, this.fertility![i] / growthMax, ch);
        } else {
          // 陸の最低気温〜最高気温を 0〜1 にして、寒色 → 中間 → 暖色
          const t = Math.min(1, Math.max(0, (temperature![i] - tmin) / Math.max(1e-6, tmax - tmin)));
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

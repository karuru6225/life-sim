// 個体の描画。ParticleContainer は少数の属性だけを毎フレーム GPU に送る軽量な描画経路。
// 種類ごとに 1 つのレイヤーを作り、肉食を草食の上に重ねる。
// 個体ごとに位置・大きさ・色（系統色のとき）が違うので、それらを毎フレーム更新する。

import { Particle, ParticleContainer, Rectangle, type Texture } from 'pixi.js';
import { DIMMED_ALPHA } from '../config';
import type { LineageSelection } from '../protocol';
import { HUE_PALETTE, hueIndex } from './palette';

/** 色分けの方法 */
export type ColorMode = 'kind' | 'lineage';

export class AgentLayer {
  readonly container: ParticleContainer;
  private readonly pool: Particle[] = [];
  /** Particle.color に入れる値（tint 経由だと毎回色変換が走って遅いので、先に変換しておく） */
  private readonly kindColor: number;
  private readonly huePalette: number[];
  /** 系統を選んでいるときに、系統の外の個体に使う薄い色 */
  private readonly kindColorDim: number;
  private readonly huePaletteDim: number[];

  constructor(
    private readonly texture: Texture,
    tint: string,
    /** テクスチャの半径に対する、大きさ 1 のときの描画倍率 */
    private readonly baseScale: number,
    /** 進行方向に合わせて回すか（三角形で描く肉食） */
    private readonly rotate: boolean,
    worldWidth: number,
    worldHeight: number,
  ) {
    this.container = new ParticleContainer({
      texture,
      dynamicProperties: { position: true, vertex: true, color: true, rotation: rotate, uvs: false },
    });
    // ParticleContainer は自分の範囲を計算しないので明示する
    this.container.boundsArea = new Rectangle(0, 0, worldWidth, worldHeight);
    this.kindColor = new Particle({ texture, tint }).color;
    this.huePalette = HUE_PALETTE.map((c) => new Particle({ texture, tint: c }).color);
    this.kindColorDim = new Particle({ texture, tint, alpha: DIMMED_ALPHA }).color;
    this.huePaletteDim = HUE_PALETTE.map((c) => new Particle({ texture, tint: c, alpha: DIMMED_ALPHA }).color);
  }

  /**
   * data: [x, y, 種類, 大きさ, 標識色, 向き] × count のうち、種類が kind の個体だけを描く。
   * 個体数は毎 tick 変わるので、表示数を合わせてから書き込む
   */
  /**
   * minScale: 描画倍率の下限（引いた表示でも小さくなりすぎないように。0 なら下限なし）。
   * lineage: 選んだ系統。この種類の系統なら、系統の外の個体を薄く描く
   */
  apply(
    data: Float32Array,
    count: number,
    stride: number,
    kind: number,
    mode: ColorMode,
    minScale = 0,
    lineage: LineageSelection | null = null,
  ): void {
    const filter = lineage && lineage.kind === kind ? lineage : null;
    let n = 0;
    for (let i = 0; i < count; i++) {
      const o = i * stride;
      if (data[o + 2] !== kind) continue;
      const p = this.particle(n++);
      p.x = data[o];
      p.y = data[o + 1];
      const s = Math.max(minScale, data[o + 3] * this.baseScale);
      p.scaleX = s;
      p.scaleY = s;
      const hue = data[o + 4];
      const outside = filter !== null && (((hue - filter.from) % 1) + 1) % 1 >= filter.width;
      if (outside) {
        p.color = mode === 'kind' ? this.kindColorDim : this.huePaletteDim[hueIndex(hue)];
      } else {
        p.color = mode === 'kind' ? this.kindColor : this.huePalette[hueIndex(hue)];
      }
      if (this.rotate) p.rotation = data[o + 5];
    }
    const children = this.container.particleChildren;
    if (children.length !== n) {
      children.length = 0;
      for (let i = 0; i < n; i++) children.push(this.pool[i]);
      this.container.update();
    }
  }

  /** 世界の大きさを変える（ParticleContainer は自分の範囲を計算しないので明示する） */
  setWorldSize(width: number, height: number): void {
    this.container.boundsArea = new Rectangle(0, 0, width, height);
  }

  private particle(i: number): Particle {
    if (i >= this.pool.length) {
      this.pool.push(new Particle({ texture: this.texture, anchorX: 0.5, anchorY: 0.5 }));
    }
    return this.pool[i];
  }
}

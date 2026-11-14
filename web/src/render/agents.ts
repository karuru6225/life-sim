// 個体の描画。ParticleContainer は少数の属性だけを毎フレーム GPU に送る軽量な描画経路。
// 種類ごとに 1 つのレイヤーを作り、肉食を草食の上に重ねる。
// 個体ごとに位置・大きさ・色（系統色のとき）が違うので、それらを毎フレーム更新する。

import { Particle, ParticleContainer, Rectangle, type Texture } from 'pixi.js';
import { HUE_PALETTE, hueIndex } from './palette';

/** 色分けの方法 */
export type ColorMode = 'kind' | 'lineage';

export class AgentLayer {
  readonly container: ParticleContainer;
  private readonly pool: Particle[] = [];
  /** Particle.color に入れる値（tint 経由だと毎回色変換が走って遅いので、先に変換しておく） */
  private readonly kindColor: number;
  private readonly huePalette: number[];

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
  }

  /**
   * data: [x, y, 種類, 大きさ, 標識色, 向き] × count のうち、種類が kind の個体だけを描く。
   * 個体数は毎 tick 変わるので、表示数を合わせてから書き込む
   */
  apply(data: Float32Array, count: number, stride: number, kind: number, mode: ColorMode): void {
    let n = 0;
    for (let i = 0; i < count; i++) {
      const o = i * stride;
      if (data[o + 2] !== kind) continue;
      const p = this.particle(n++);
      p.x = data[o];
      p.y = data[o + 1];
      const s = data[o + 3] * this.baseScale;
      p.scaleX = s;
      p.scaleY = s;
      p.color = mode === 'kind' ? this.kindColor : this.huePalette[hueIndex(data[o + 4])];
      if (this.rotate) p.rotation = data[o + 5];
    }
    const children = this.container.particleChildren;
    if (children.length !== n) {
      children.length = 0;
      for (let i = 0; i < n; i++) children.push(this.pool[i]);
      this.container.update();
    }
  }

  private particle(i: number): Particle {
    if (i >= this.pool.length) {
      this.pool.push(new Particle({ texture: this.texture, anchorX: 0.5, anchorY: 0.5 }));
    }
    return this.pool[i];
  }
}

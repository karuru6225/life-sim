// 個体の描画。ParticleContainer は位置だけを毎フレーム GPU に送る軽量な描画経路。
// 種類ごとに 1 つのレイヤーを作る（大きさと色を種類で固定できるので、位置だけ更新すればいい）。

import { Particle, ParticleContainer, Rectangle, type Texture } from 'pixi.js';

export class AgentLayer {
  readonly container: ParticleContainer;
  private readonly pool: Particle[] = [];

  constructor(
    private readonly texture: Texture,
    private readonly tint: string,
    worldWidth: number,
    worldHeight: number,
  ) {
    this.container = new ParticleContainer({
      texture,
      dynamicProperties: { position: true, color: false, rotation: false, vertex: false, uvs: false },
    });
    // ParticleContainer は自分の範囲を計算しないので明示する
    this.container.boundsArea = new Rectangle(0, 0, worldWidth, worldHeight);
  }

  /**
   * data: [x, y, 種類] × count のうち、種類が kind の個体だけを描く。
   * 個体数は毎 tick 変わるので、表示数を合わせてから位置を書き込む
   */
  apply(data: Float32Array, count: number, stride: number, kind: number): void {
    let n = 0;
    for (let i = 0; i < count; i++) {
      const o = i * stride;
      if (data[o + 2] !== kind) continue;
      const p = this.particle(n++);
      p.x = data[o];
      p.y = data[o + 1];
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
      this.pool.push(new Particle({ texture: this.texture, anchorX: 0.5, anchorY: 0.5, tint: this.tint }));
    }
    return this.pool[i];
  }
}

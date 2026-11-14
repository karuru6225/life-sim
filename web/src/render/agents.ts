// 個体の描画。ParticleContainer は位置と色だけを毎フレーム GPU に送る軽量な描画経路。

import { Color, Particle, ParticleContainer, Rectangle, type Texture } from 'pixi.js';
import { COLORS, DENSITY_LEVELS } from '../config';

export class AgentLayer {
  readonly container: ParticleContainer;
  private particles: Particle[] = [];
  /** 近傍数 → Particle.color に入れる値（tint 経由だと毎回色変換が走って遅い） */
  private readonly palette: number[];

  constructor(
    private readonly texture: Texture,
    worldWidth: number,
    worldHeight: number,
  ) {
    this.container = new ParticleContainer({
      texture,
      dynamicProperties: { position: true, color: true, rotation: false, vertex: false, uvs: false },
    });
    // ParticleContainer は自分の範囲を計算しないので明示する
    this.container.boundsArea = new Rectangle(0, 0, worldWidth, worldHeight);
    this.palette = densityRamp().map((tint) => new Particle({ texture, tint }).color);
  }

  get count(): number {
    return this.particles.length;
  }

  resize(count: number): void {
    if (count === this.particles.length) return;
    const children = this.container.particleChildren;
    children.length = 0;
    this.particles = [];
    for (let i = 0; i < count; i++) {
      const p = new Particle({ texture: this.texture, anchorX: 0.5, anchorY: 0.5 });
      this.particles.push(p);
      children.push(p);
    }
    this.container.update();
  }

  /** data: [x, y, 近傍数] × count */
  apply(data: Float32Array, count: number, stride: number): void {
    const n = Math.min(count, this.particles.length);
    const last = this.palette.length - 1;
    for (let i = 0; i < n; i++) {
      const p = this.particles[i];
      const o = i * stride;
      p.x = data[o];
      p.y = data[o + 1];
      p.color = this.palette[Math.min(data[o + 2] | 0, last)];
    }
  }
}

/** 密度の色の段階を、COLORS.densityStops の間を線形補間して作る */
export function densityRamp(): string[] {
  // toRgbArray() は各成分を 0〜1 で返し、Color は 0〜1 の配列をそのまま受け取る
  const stops = COLORS.densityStops.map((c) => new Color(c).toRgbArray());
  const out: string[] = [];
  for (let i = 0; i < DENSITY_LEVELS; i++) {
    const t = (i / (DENSITY_LEVELS - 1)) * (stops.length - 1);
    const k = Math.min(Math.floor(t), stops.length - 2);
    const f = t - k;
    const a = stops[k];
    const b = stops[k + 1];
    out.push(new Color([0, 1, 2].map((ch) => a[ch] + (b[ch] - a[ch]) * f)).toHex());
  }
  return out;
}

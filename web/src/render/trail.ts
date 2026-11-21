// 選択中の個体の移動の履歴。選択してからの位置を線でつなぎ、選んだ場所に小さな輪を描く。
// 世界はトーラスなので、端をまたいだところ（1 tick で世界の半分より大きく飛んだところ）は線を切る。

import { Graphics } from 'pixi.js';
import { COLORS } from '../config';

/** 線の下に敷く縁取りの色。明るい地面（砂地・草地）の上でも線が埋もれないようにする */
const CASING = 0x0b1416;

export class Trail {
  readonly graphics = new Graphics();
  /** 位置（x, y の組を古い順） */
  private points: number[] = [];
  /** 最後に描いたときの位置の数とカメラの倍率（変わらなければ描き直さない） */
  private drawnCount = -1;
  private drawnScale = 0;

  /** worldSize は今の世界の広さ（幅, 高さ）。世界の広さは作り直しで変わるので、描くたびに聞く */
  constructor(private readonly worldSize: () => [number, number]) {}

  /** 位置の数 */
  get count(): number {
    return this.points.length / 2;
  }

  /** start 番目からの位置を足す。start が 0 なら最初からやり直す */
  add(start: number, points: number[]): void {
    if (start === 0) this.points = [];
    for (const v of points) this.points.push(v);
  }

  clear(): void {
    this.points = [];
    this.graphics.clear();
    this.drawnCount = -1;
  }

  /** 線の太さを画面上で一定にするため、カメラの倍率 scale で描く */
  draw(scale: number): void {
    const n = this.count;
    if (n === this.drawnCount && scale === this.drawnScale) return;
    this.drawnCount = n;
    this.drawnScale = scale;
    const g = this.graphics;
    g.clear();
    if (n === 0) return;
    const p = this.points;
    const [width, height] = this.worldSize();
    const path = () => {
      g.moveTo(p[0], p[1]);
      for (let k = 1; k < n; k++) {
        const x = p[2 * k];
        const y = p[2 * k + 1];
        const jump = Math.abs(x - p[2 * k - 2]) > width / 2 || Math.abs(y - p[2 * k - 1]) > height / 2;
        if (jump) g.moveTo(x, y);
        else g.lineTo(x, y);
      }
    };
    path();
    g.stroke({ width: 3.5 / scale, color: CASING, alpha: 0.55, join: 'round', cap: 'round' });
    path();
    g.stroke({ width: 1.5 / scale, color: COLORS.marker, alpha: 0.9, join: 'round', cap: 'round' });
    // 選んだ場所
    g.circle(p[0], p[1], 3 / scale).fill({ color: CASING, alpha: 0.55 });
    g.circle(p[0], p[1], 3 / scale).stroke({ width: 1.5 / scale, color: COLORS.marker });
  }
}

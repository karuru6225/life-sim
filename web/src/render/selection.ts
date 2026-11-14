// 選択中の個体の目印。個体を囲む輪と、視野の範囲を示す円を描く。

import { Graphics } from 'pixi.js';
import { COLORS } from '../config';

export class SelectionMarker {
  readonly graphics = new Graphics();

  /** 選択中の個体の位置・大きさ・視野で描き直す。scale はカメラの倍率（線の太さを画面上で一定にする） */
  update(x: number, y: number, bodyRadius: number, vision: number, scale: number): void {
    const g = this.graphics;
    g.clear();
    g.circle(x, y, vision).stroke({ width: 1 / scale, color: COLORS.marker, alpha: 0.45 });
    // 体との間に画面上で 4px の隙間を空ける
    g.circle(x, y, bodyRadius + 4 / scale).stroke({ width: 2 / scale, color: COLORS.marker });
    g.visible = true;
  }

  hide(): void {
    this.graphics.clear();
    this.graphics.visible = false;
  }
}

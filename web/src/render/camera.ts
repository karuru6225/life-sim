// カメラ操作。ドラッグで移動、ホイールでカーソル位置を中心にズーム、ダブルクリックで全体表示。

import type { Container } from 'pixi.js';

const MAX_SCALE = 24;

export class Camera {
  private dragging = false;
  private lastX = 0;
  private lastY = 0;

  constructor(
    private readonly target: Container,
    private readonly canvas: HTMLCanvasElement,
    private readonly worldWidth: number,
    private readonly worldHeight: number,
  ) {
    canvas.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    canvas.addEventListener('pointerdown', (e) => {
      this.dragging = true;
      this.lastX = e.clientX;
      this.lastY = e.clientY;
      canvas.setPointerCapture(e.pointerId);
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!this.dragging) return;
      this.target.position.x += e.clientX - this.lastX;
      this.target.position.y += e.clientY - this.lastY;
      this.lastX = e.clientX;
      this.lastY = e.clientY;
    });
    const stop = () => (this.dragging = false);
    canvas.addEventListener('pointerup', stop);
    canvas.addEventListener('pointercancel', stop);
    canvas.addEventListener('dblclick', () => this.fit());
  }

  /** 世界全体が画面に収まる倍率 */
  private fitScale(): number {
    return Math.min(this.canvas.clientWidth / this.worldWidth, this.canvas.clientHeight / this.worldHeight) * 0.96;
  }

  fit(): void {
    const s = this.fitScale();
    this.target.scale.set(s);
    this.target.position.set(
      (this.canvas.clientWidth - this.worldWidth * s) / 2,
      (this.canvas.clientHeight - this.worldHeight * s) / 2,
    );
  }

  private onWheel(e: WheelEvent): void {
    e.preventDefault();
    const rect = this.canvas.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    const s = this.target.scale.x;
    const next = Math.min(MAX_SCALE, Math.max(this.fitScale() * 0.5, s * Math.exp(-e.deltaY * 0.0015)));
    // カーソルの下にある世界座標が、ズーム後も同じ位置に来るようにする
    const wx = (px - this.target.position.x) / s;
    const wy = (py - this.target.position.y) / s;
    this.target.scale.set(next);
    this.target.position.set(px - wx * next, py - wy * next);
  }
}

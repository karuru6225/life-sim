// カメラ操作。ドラッグで移動、ホイールでカーソル位置を中心にズーム、ダブルクリックで全体表示。
// 押してから離すまでの移動が小さければクリックとして扱う。

import type { Container } from 'pixi.js';

const MAX_SCALE = 24;
/** これ未満の移動で離したらクリックとみなす（px） */
const CLICK_SLOP = 5;

export interface CameraHandlers {
  /** クリックされた画面上の位置（キャンバス左上基準の px） */
  onClick?(px: number, py: number): void;
  /** ユーザーがドラッグで視点を動かした */
  onPan?(): void;
}

export class Camera {
  private dragging = false;
  private lastX = 0;
  private lastY = 0;
  private moved = 0;

  constructor(
    private readonly target: Container,
    private readonly canvas: HTMLCanvasElement,
    private readonly worldWidth: number,
    private readonly worldHeight: number,
    private readonly handlers: CameraHandlers = {},
  ) {
    canvas.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    canvas.addEventListener('pointerdown', (e) => {
      this.dragging = true;
      this.moved = 0;
      this.lastX = e.clientX;
      this.lastY = e.clientY;
      canvas.setPointerCapture(e.pointerId);
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!this.dragging) return;
      const dx = e.clientX - this.lastX;
      const dy = e.clientY - this.lastY;
      this.moved += Math.abs(dx) + Math.abs(dy);
      this.target.position.x += dx;
      this.target.position.y += dy;
      this.lastX = e.clientX;
      this.lastY = e.clientY;
      if (this.moved >= CLICK_SLOP) this.handlers.onPan?.();
    });
    canvas.addEventListener('pointerup', (e) => {
      if (!this.dragging) return;
      this.dragging = false;
      if (this.moved < CLICK_SLOP) {
        const rect = canvas.getBoundingClientRect();
        this.handlers.onClick?.(e.clientX - rect.left, e.clientY - rect.top);
      }
    });
    canvas.addEventListener('pointercancel', () => (this.dragging = false));
    canvas.addEventListener('dblclick', () => this.fit());
  }

  get scale(): number {
    return this.target.scale.x;
  }

  /** 画面上の位置 → 世界座標 */
  toWorld(px: number, py: number): { x: number; y: number } {
    return {
      x: (px - this.target.position.x) / this.scale,
      y: (py - this.target.position.y) / this.scale,
    };
  }

  /** 世界座標 (x, y) が画面の中央に来るように動かす */
  centerOn(x: number, y: number): void {
    this.target.position.set(this.canvas.clientWidth / 2 - x * this.scale, this.canvas.clientHeight / 2 - y * this.scale);
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
    const s = this.scale;
    const next = Math.min(MAX_SCALE, Math.max(this.fitScale() * 0.5, s * Math.exp(-e.deltaY * 0.0015)));
    // カーソルの下にある世界座標が、ズーム後も同じ位置に来るようにする
    const wx = (px - this.target.position.x) / s;
    const wy = (py - this.target.position.y) / s;
    this.target.scale.set(next);
    this.target.position.set(px - wx * next, py - wy * next);
  }
}

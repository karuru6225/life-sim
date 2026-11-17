// 地図の上に浮かぶウインドウ（統計・脳）の共通の動き: 見出しをドラッグして動かす、右下の角で大きさを変える、
// 位置と大きさをブラウザに覚えておく。

/**
 * root を浮かぶウインドウにする。head（見出し）をドラッグすると動き、位置と大きさは key で覚えておく。
 * 大きさが変わったら onResize を呼ぶ（グラフの描き直し用）
 */
export function makeFloating(root: HTMLElement, head: HTMLElement, key: string, onResize: () => void): void {
  const save = () => {
    if (root.hidden) return;
    const r = root.getBoundingClientRect();
    try {
      localStorage.setItem(key, JSON.stringify({ left: r.left, top: r.top, width: r.width, height: r.height }));
    } catch {
      // 覚えておけなくても使える
    }
  };

  head.addEventListener('pointerdown', (e) => {
    if ((e.target as HTMLElement).closest('button, input, label')) return;
    const rect = root.getBoundingClientRect();
    const dx = e.clientX - rect.left;
    const dy = e.clientY - rect.top;
    head.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const x = Math.min(Math.max(0, ev.clientX - dx), window.innerWidth - 80);
      const y = Math.min(Math.max(0, ev.clientY - dy), window.innerHeight - 40);
      root.style.left = `${x}px`;
      root.style.top = `${y}px`;
    };
    const up = () => {
      head.removeEventListener('pointermove', move);
      head.removeEventListener('pointerup', up);
      save();
    };
    head.addEventListener('pointermove', move);
    head.addEventListener('pointerup', up);
  });

  try {
    const g = JSON.parse(localStorage.getItem(key) ?? 'null') as Record<string, number> | null;
    if (g) {
      const width = Math.min(g.width, window.innerWidth - 16);
      const height = Math.min(g.height, window.innerHeight - 16);
      root.style.width = `${width}px`;
      root.style.height = `${height}px`;
      root.style.left = `${Math.min(Math.max(0, g.left), window.innerWidth - width)}px`;
      root.style.top = `${Math.min(Math.max(0, g.top), window.innerHeight - height)}px`;
    }
  } catch {
    // 既定の位置のまま
  }

  new ResizeObserver(() => {
    save();
    onResize();
  }).observe(root);
}

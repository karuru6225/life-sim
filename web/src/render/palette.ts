// 系統色（標識色の遺伝子）のパレット。
// 色相ごとに明度を調整して、全色の相対輝度をそろえる。
// 色相環をそのまま使うと青や紫が暗く、植物の上で見えにくくなるため。

import { HUE_LUMINANCE, HUE_STEPS } from '../config';

/** WCAG の相対輝度 */
export function luminance(r: number, g: number, b: number): number {
  const lin = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h * 12) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0), f(8), f(4)];
}

/** 色相 h（0〜1）で、相対輝度が target になる色（0〜1 の RGB） */
function hueAtLuminance(h: number, target: number): [number, number, number] {
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (luminance(...hslToRgb(h, 0.65, mid)) < target) lo = mid;
    else hi = mid;
  }
  return hslToRgb(h, 0.65, (lo + hi) / 2);
}

const toHex = (rgb: [number, number, number]) =>
  '#' + rgb.map((v) => Math.round(v * 255).toString(16).padStart(2, '0')).join('');

/** 色相を HUE_STEPS 段階に分けた色（#rrggbb） */
export const HUE_PALETTE: string[] = Array.from({ length: HUE_STEPS }, (_, i) =>
  toHex(hueAtLuminance(i / HUE_STEPS, HUE_LUMINANCE)),
);

/** 標識色の値（0〜1）→ パレットの番号 */
export function hueIndex(hue: number): number {
  return Math.min(HUE_STEPS - 1, Math.floor(hue * HUE_STEPS));
}

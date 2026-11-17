// 脳のウインドウ。選択中の個体の脳の、すべてのニューロンとつながりを、今の活動とともに描く。
//
// 列は左から 入力 → 中間 1 層目 → 中間 2 層目 → 出力。出力は中間の両方の層から受け取る。
// 点: 今の活動。色が正負（暖色が正、寒色が負）、明るさが強さ。使っていない中間ニューロンは中抜きの小さな点
// 線: 「信号」モードでは 重み × 送り手の今の活動（今この瞬間に判断を動かしているつながり）、「重み」モードでは重みそのもの。
//     色が正負、太さと濃さが強さ。強い順に上位の割合だけを描く（全部描くと線が多すぎて読めない）
// 点にマウスを乗せると、その点につながる線だけを（割合によらず）描き、値を表示する。

import { CARNIVORE, INSPECT } from '../protocol';
import { makeFloating } from './floating';
import { INPUT_LABELS, OUTPUT_LABELS } from './inspector';

const GEOMETRY_KEY = 'life-sim.brainWindow';

/** 正と負の色（パネルの地色に対してどちらも 3:1 以上、色覚の違いでも見分けられることを検証済み） */
const POS = '#e66767';
const NEG = '#3987e5';
const PANEL = '#0c1215';
const AXIS = '#56696f';
const MUTED = '#93a3a2';
const TEXT = '#dde6e4';
const FONT = "11px ui-monospace, 'Cascadia Mono', Consolas, monospace";
const LABEL_FONT = "11px system-ui, 'Segoe UI', 'Yu Gothic UI', sans-serif";

type Mode = 'signal' | 'weight';

/** 列: 0 = 入力、1 = 中間 1 層目、2 = 中間 2 層目、3 = 出力 */
interface NodeRef {
  col: number;
  idx: number;
}

interface Edge {
  from: NodeRef;
  to: NodeRef;
  weight: number;
}

/** 脳の構造と重み（genome.rs の brain_part() の並びを読んだもの） */
interface Net {
  nIn: number;
  h1: number;
  h2: number;
  nOut: number;
  /** 中間ニューロンを使っているか（1 層目のあとに 2 層目） */
  active: boolean[];
  /** バイアス [列][番号]（入力は 0） */
  bias: number[][];
  edges: Edge[];
}

/** 重みの並びを読む。長さが構造と合わなければ null */
function parseNet(v: number[], nIn: number, h1: number, h2: number, nOut: number): Net | null {
  const nHidden = h1 + h2;
  const len = nHidden + h1 * (nIn + 1) + h2 * (h1 + 1) + nOut * (nHidden + 1);
  return v.length === len ? build(v, nIn, h1, h2, nOut) : null;
}

function build(v: number[], nIn: number, h1: number, h2: number, nOut: number): Net {
  const nHidden = h1 + h2;
  const active = Array.from({ length: nHidden }, (_, h) => v[h] > 0);
  const w1 = nHidden;
  const w2 = w1 + h1 * (nIn + 1);
  const w3 = w2 + h2 * (h1 + 1);
  const bias: number[][] = [new Array(nIn).fill(0), new Array(h1).fill(0), new Array(h2).fill(0), new Array(nOut).fill(0)];
  const edges: Edge[] = [];
  for (let h = 0; h < h1; h++) {
    const row = w1 + h * (nIn + 1);
    bias[1][h] = v[row + nIn];
    if (!active[h]) continue;
    for (let k = 0; k < nIn; k++) {
      if (v[row + k] !== 0) edges.push({ from: { col: 0, idx: k }, to: { col: 1, idx: h }, weight: v[row + k] });
    }
  }
  for (let h = 0; h < h2; h++) {
    const row = w2 + h * (h1 + 1);
    bias[2][h] = v[row + h1];
    if (!active[h1 + h]) continue;
    for (let k = 0; k < h1; k++) {
      if (active[k] && v[row + k] !== 0) edges.push({ from: { col: 1, idx: k }, to: { col: 2, idx: h }, weight: v[row + k] });
    }
  }
  for (let o = 0; o < nOut; o++) {
    const row = w3 + o * (nHidden + 1);
    bias[3][o] = v[row + nHidden];
    for (let h = 0; h < nHidden; h++) {
      if (!active[h] || v[row + h] === 0) continue;
      const from = h < h1 ? { col: 1, idx: h } : { col: 2, idx: h - h1 };
      edges.push({ from, to: { col: 3, idx: o }, weight: v[row + h] });
    }
  }
  return { nIn, h1, h2, nOut, active, bias, edges };
}

function softsign(x: number): number {
  return x / (1 + Math.abs(x));
}

function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** 地色から正負の色へ、強さ t（0〜1）で混ぜた色 */
function signColor(value: number, t: number): string {
  const [br, bg, bb] = rgb(PANEL);
  const [cr, cg, cb] = rgb(value >= 0 ? POS : NEG);
  const k = Math.min(1, Math.max(0, t));
  return `rgb(${Math.round(br + (cr - br) * k)}, ${Math.round(bg + (cg - bg) * k)}, ${Math.round(bb + (cb - bb) * k)})`;
}

function fmt(v: number): string {
  if (!Number.isFinite(v)) return '–';
  return (v >= 0 ? ' ' : '') + v.toFixed(2);
}

export class BrainWindow {
  private readonly root: HTMLElement;
  private readonly el: Record<string, HTMLElement> = {};
  private mode: Mode = 'signal';
  /** 描く線の割合（強い順の上位、0〜1） */
  private share = 0.3;
  private net: Net | null = null;
  private netId = 0;
  /** 今の活動 [列][番号]。入力はそのまま、出力は softsign をかけた値 */
  private act: number[][] | null = null;
  /** 出力の softsign をかける前の値（表示用） */
  private rawOut: number[] = [];
  private who = '';
  private hover: NodeRef | null = null;
  /** 点の位置 [列][番号] */
  private pos: { x: number; y: number }[][] = [];

  constructor(
    root: HTMLElement,
    private readonly onOpenChange: (open: boolean) => void,
  ) {
    this.root = root;
    root.className = 'stats-win brain-win';
    root.hidden = true;
    root.setAttribute('role', 'dialog');
    root.setAttribute('aria-label', '脳');
    root.innerHTML = `
      <header class="stats-head" data-k="head">
        <span class="stats-title">脳</span>
        <span class="brain-who" data-k="who">–</span>
        <button type="button" class="stats-close" data-k="close" aria-label="脳のウインドウを閉じる" title="閉じる">×</button>
      </header>
      <div class="stats-range brain-controls">
        <span class="group-label">線</span>
        <button type="button" data-mode="signal" title="重み × 送り手の今の活動。今この瞬間に判断を動かしているつながり">信号</button>
        <button type="button" data-mode="weight" title="重みそのもの（生まれつき決まっていて、生涯変わらない）">重み</button>
        <label class="brain-share">強い順に上位 <input type="range" min="0.02" max="1" step="0.02" data-k="share" /> <span data-k="shareValue">–</span></label>
      </div>
      <canvas class="brain-canvas" data-k="canvas" aria-label="選択中の個体の脳のニューロンとつながり"></canvas>
      <p class="stats-note brain-legend">
        <span><i class="swatch" style="background:${POS}"></i>正（強める）</span>
        <span><i class="swatch" style="background:${NEG}"></i>負（弱める）</span>
        <span>点: 今の活動（明るいほど強い。中抜きは使っていない中間ニューロン）</span>
        <span>線: 太く濃いほど強い</span>
        <span>点にマウスを乗せると、そのつながりだけを見る</span>
      </p>
      <div class="stats-tip" data-k="tip" hidden></div>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) this.el[node.dataset.k!] = node;
    this.el.close.addEventListener('click', () => this.setOpen(false));
    for (const b of root.querySelectorAll<HTMLButtonElement>('[data-mode]')) {
      b.addEventListener('click', () => {
        this.mode = b.dataset.mode as Mode;
        this.sync();
        this.draw();
      });
    }
    const share = this.el.share as HTMLInputElement;
    share.value = String(this.share);
    share.addEventListener('input', () => {
      this.share = Number(share.value);
      this.sync();
      this.draw();
    });
    const canvas = this.el.canvas as HTMLCanvasElement;
    canvas.addEventListener('mousemove', (e) => {
      const rect = canvas.getBoundingClientRect();
      const hit = this.nodeAt(e.clientX - rect.left, e.clientY - rect.top);
      const changed = hit?.col !== this.hover?.col || hit?.idx !== this.hover?.idx;
      this.hover = hit;
      if (changed) this.draw();
      this.showTip(e);
    });
    canvas.addEventListener('mouseleave', () => {
      this.hover = null;
      this.el.tip.hidden = true;
      this.draw();
    });
    makeFloating(root, this.el.head, GEOMETRY_KEY, () => this.draw());
    this.sync();
  }

  get isOpen(): boolean {
    return !this.root.hidden;
  }

  setOpen(open: boolean): void {
    this.root.hidden = !open;
    if (open) this.draw();
    this.onOpenChange(open);
  }

  toggle(): void {
    this.setOpen(!this.isOpen);
  }

  /** 選んだ個体の重み（選んだ個体が変わったときに届く。空なら個体がいない） */
  setNet(id: number, values: number[], nIn: number, h1: number, h2: number, nOut: number): void {
    this.netId = id;
    this.net = values.length > 0 ? parseNet(values, nIn, h1, h2, nOut) : null;
    this.act = null;
    this.hover = null;
    this.draw();
  }

  /** 毎フレームの、選択中の個体の脳の値と詳細（INSPECT の並び）。選択していなければ null */
  update(brain: { input: number[]; output: number[]; hidden: number[]; h1: number } | null, inspect: number[] | null, tick: number): void {
    if (!brain || !inspect || inspect[INSPECT.status] === 0) {
      this.act = null;
      this.who = '';
      if (this.isOpen) this.draw();
      return;
    }
    const id = inspect[INSPECT.id];
    if (id !== this.netId) return; // 重みがまだ届いていない
    const alive = inspect[INSPECT.status] === 1;
    const kind = inspect[INSPECT.kind] === CARNIVORE ? '肉食' : '草食';
    this.who = `${kind} #${id}${alive ? '' : '（死亡。最後の状態）'} · tick ${tick.toLocaleString('ja-JP')}`;
    if (alive) {
      this.rawOut = brain.output;
      this.act = [brain.input, brain.hidden.slice(0, brain.h1), brain.hidden.slice(brain.h1), brain.output.map(softsign)];
    }
    if (this.isOpen) this.draw();
  }

  private sync(): void {
    for (const b of this.root.querySelectorAll<HTMLButtonElement>('[data-mode]')) {
      const on = b.dataset.mode === this.mode;
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
    this.el.shareValue.textContent = `${Math.round(this.share * 100)}%`;
  }

  /** 線の値（モードによる） */
  private edgeValue(e: Edge): number {
    if (this.mode === 'weight' || !this.act) return e.weight;
    return e.weight * (this.act[e.from.col][e.from.idx] ?? 0);
  }

  private nodeAt(x: number, y: number): NodeRef | null {
    let best: NodeRef | null = null;
    let bestD = 10 * 10;
    this.pos.forEach((col, c) =>
      col.forEach((p, i) => {
        const d = (p.x - x) ** 2 + (p.y - y) ** 2;
        if (d < bestD) {
          bestD = d;
          best = { col: c, idx: i };
        }
      }),
    );
    return best;
  }

  private draw(): void {
    if (!this.isOpen) return;
    this.el.who.textContent = this.who || '–';
    const canvas = this.el.canvas as HTMLCanvasElement;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const net = this.net;
    if (!net || !this.act) {
      this.pos = [];
      ctx.fillStyle = MUTED;
      ctx.font = LABEL_FONT;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(this.who ? '脳の値を待っています' : '地図で個体を選ぶと、その脳が表示される', w / 2, h / 2);
      return;
    }
    const act = this.act;

    // 点の位置。左右に名前と値の欄をとり、残りを 4 列に分ける
    ctx.font = LABEL_FONT;
    const nameW = (labels: string[]) => Math.max(...labels.map((l) => ctx.measureText(l).width));
    ctx.font = FONT;
    const valueW = ctx.measureText('-0.00').width;
    ctx.font = LABEL_FONT;
    const left = valueW + 8 + nameW(INPUT_LABELS) + 12;
    const right = w - (valueW + 8 + nameW(OUTPUT_LABELS) + 12);
    const top = 10;
    const bottom = h - 10;
    const xs = [left, left + (right - left) / 3, left + ((right - left) * 2) / 3, right];
    const counts = [net.nIn, net.h1, net.h2, net.nOut];
    this.pos = counts.map((n, c) =>
      Array.from({ length: n }, (_, i) => ({ x: xs[c], y: top + ((i + 0.5) * (bottom - top)) / n })),
    );

    // 線: 値の強い順に、上位の割合だけ（マウスを乗せた点があれば、その点につながる線を全部）
    const hv = this.hover;
    const touches = (e: Edge) => !!hv && ((e.from.col === hv.col && e.from.idx === hv.idx) || (e.to.col === hv.col && e.to.idx === hv.idx));
    const valued = net.edges.map((e) => ({ e, v: this.edgeValue(e) })).filter((x) => x.v !== 0);
    valued.sort((a, b) => Math.abs(a.v) - Math.abs(b.v));
    const shown = hv ? valued.filter((x) => touches(x.e)) : valued.slice(Math.floor(valued.length * (1 - this.share)));
    const maxAbs = Math.max(1e-6, ...shown.map((x) => Math.abs(x.v)));
    ctx.lineCap = 'round';
    for (const { e, v } of shown) {
      const t = Math.abs(v) / maxAbs;
      const a = this.pos[e.from.col][e.from.idx];
      const b = this.pos[e.to.col][e.to.idx];
      ctx.strokeStyle = v >= 0 ? POS : NEG;
      ctx.globalAlpha = 0.15 + 0.85 * t;
      ctx.lineWidth = 0.5 + 3 * t;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      // 1 層目から出力への線は 2 層目の列を通るので、少し曲げて 2 層目の線と見分ける
      if (e.from.col === 1 && e.to.col === 3) {
        ctx.quadraticCurveTo((a.x + b.x) / 2, (a.y + b.y) / 2 - (bottom - top) * 0.04, b.x, b.y);
      } else {
        ctx.lineTo(b.x, b.y);
      }
      ctx.stroke();
    }
    ctx.globalAlpha = 1;

    // 点
    for (let c = 0; c < 4; c++) {
      for (let i = 0; i < counts[c]; i++) {
        const p = this.pos[c][i];
        const used = c === 0 || c === 3 || net.active[c === 1 ? i : net.h1 + i];
        const v = act[c][i] ?? 0;
        const isHover = hv?.col === c && hv?.idx === i;
        ctx.beginPath();
        if (!used) {
          ctx.arc(p.x, p.y, 3, 0, Math.PI * 2);
          ctx.strokeStyle = AXIS;
          ctx.lineWidth = 1;
          ctx.stroke();
          continue;
        }
        ctx.arc(p.x, p.y, isHover ? 7 : 5, 0, Math.PI * 2);
        ctx.fillStyle = signColor(v, Math.min(1, Math.abs(v)));
        ctx.fill();
        ctx.strokeStyle = isHover ? TEXT : AXIS;
        ctx.lineWidth = isHover ? 2 : 1;
        ctx.stroke();
      }
    }

    // 名前と値（入力は左、出力は右）。点の間隔が文字より狭いときは名前を省く
    ctx.textBaseline = 'middle';
    const label = (c: number, labels: string[], values: number[]) => {
      const n = counts[c];
      const gap = (bottom - top) / n;
      for (let i = 0; i < n; i++) {
        const p = this.pos[c][i];
        const dim = hv && !(hv.col === c && hv.idx === i) && !net.edges.some((e) => touches(e) && ((e.from.col === c && e.from.idx === i) || (e.to.col === c && e.to.idx === i)));
        ctx.globalAlpha = dim ? 0.45 : 1;
        if (gap >= 10) {
          ctx.font = LABEL_FONT;
          ctx.fillStyle = MUTED;
          ctx.textAlign = c === 0 ? 'right' : 'left';
          ctx.fillText(labels[i] ?? `#${i}`, c === 0 ? p.x - 10 : p.x + 10, p.y);
        }
        ctx.font = FONT;
        ctx.fillStyle = TEXT;
        ctx.textAlign = c === 0 ? 'left' : 'right';
        ctx.fillText(fmt(values[i] ?? 0), c === 0 ? 2 : w - 2, p.y);
      }
      ctx.globalAlpha = 1;
    };
    label(0, INPUT_LABELS, act[0]);
    label(3, OUTPUT_LABELS, this.rawOut);
  }

  private showTip(e: MouseEvent): void {
    const tip = this.el.tip;
    const hv = this.hover;
    const net = this.net;
    if (!hv || !net || !this.act) {
      tip.hidden = true;
      return;
    }
    const v = this.act[hv.col][hv.idx] ?? 0;
    const name =
      hv.col === 0
        ? `入力: ${INPUT_LABELS[hv.idx] ?? hv.idx}`
        : hv.col === 3
          ? `出力: ${OUTPUT_LABELS[hv.idx] ?? hv.idx}`
          : `中間 ${hv.col} 層目 #${hv.idx + 1}`;
    const used = hv.col === 0 || hv.col === 3 || net.active[hv.col === 1 ? hv.idx : net.h1 + hv.idx];
    const ins = net.edges.filter((x) => x.to.col === hv.col && x.to.idx === hv.idx).length;
    const outs = net.edges.filter((x) => x.from.col === hv.col && x.from.idx === hv.idx).length;
    const value = hv.col === 3 ? `${fmt(this.rawOut[hv.idx] ?? 0).trim()}（softsign ${fmt(v).trim()}）` : fmt(v).trim();
    tip.textContent = used
      ? `${name} · 値 ${value}${hv.col > 0 ? ` · バイアス ${fmt(net.bias[hv.col][hv.idx]).trim()}` : ''} · 受ける線 ${ins} · 送る線 ${outs}`
      : `${name} · 使っていない（遺伝子で止まっている）`;
    tip.hidden = false;
    const box = this.root.getBoundingClientRect();
    const x = e.clientX - box.left + 12;
    tip.style.left = `${Math.max(4, Math.min(x, box.width - tip.offsetWidth - 8))}px`;
    tip.style.top = `${e.clientY - box.top + 14}px`;
  }
}

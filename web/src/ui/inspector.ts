// 選択中の個体の詳細パネル。

import { COLORS, TICK_HZ } from '../config';
import { CARNIVORE, DEATH, HERBIVORE, INSPECT, STATE } from '../protocol';
import { HUE_PALETTE, hueIndex } from '../render/palette';

export interface InspectorHandlers {
  onClose(): void;
  onToggleFollow(): void;
  /** 選択中の個体の系統（標識色の近い個体）の統計を見る */
  onShowLineage(): void;
  /** id の個体を選び直す（親をたどるとき） */
  onSelectId(id: number): void;
}

/** 脳の入力の名前（sim/src/genome.rs の IN_* の並び） */
const INPUT_LABELS = [
  'エネルギー', '足元の草', '草の方向・前', '草の方向・右', '天敵・前', '天敵・右', '天敵の近さ',
  '獲物・前', '獲物・右', '獲物の近さ', '仲間・前', '仲間・右', '仲間の近さ', '仲間の数',
  '仲間の向き・前', '仲間の向き・右', '興奮した仲間', '近い系統・前', '近い系統・右', '近い系統の割合',
  '繁殖できる', '繁殖相手の近さ', '前方が水', '獲物の大きさ比', '天敵の大きさ比', '植生の合い具合',
  '気温の差', '季節', '仲間のシグナル', '呼び声・前', '呼び声・右', '呼び声の大きさ', '死角の見張り',
  '足元の死体', '死体の方向・前', '死体の方向・右', '死体の新しさ',
];
/** 脳の出力の名前（sim/src/genome.rs の OUT_* の並び） */
const OUTPUT_LABELS = [
  '優先: 食べる', '優先: 繁殖', '優先: 逃げる', '優先: 仲間', '優先: 自由', '自由な方向・前', '自由な方向・右',
  '速さ', '繁殖する', '食べる', '警戒', 'シグナル',
];

const fmtInt = new Intl.NumberFormat('ja-JP');
/** エネルギーの推移に残す tick 数（sim/src/world.rs の ENERGY_HISTORY） */
const ENERGY_HISTORY = 300;

const STATE_LABEL: Record<number, Record<number, string>> = {
  [HERBIVORE]: {
    [STATE.GRAZE]: '草を食べている',
    [STATE.SEEK]: '食べずに移動している',
    [STATE.FLEE]: '肉食が見えている',
    [STATE.COURT]: '繁殖相手に向かっている',
    [STATE.WANDER]: '歩き回っている',
  },
  [CARNIVORE]: {
    [STATE.WANDER]: '獲物が見えない',
    [STATE.CHASE]: '獲物が見えている',
    [STATE.EAT]: '獲物を食べた',
    [STATE.MISSED]: '襲ったが逃げられた',
    [STATE.COURT]: '繁殖相手に向かっている',
    [STATE.SCAVENGE]: '死体を食べている',
  },
};

const DEATH_LABEL: Record<number, string> = {
  [DEATH.STARVED]: '餓死',
  [DEATH.OLD_AGE]: '寿命',
  [DEATH.EATEN]: '肉食に食べられた',
  [DEATH.INJURED]: 'けが（獲物の反撃か、死体の奪い合い）',
  [DEATH.SICK]: '腐った死体を食べて病気になった',
};

export class Inspector {
  private readonly el: Record<string, HTMLElement> = {};
  /** 脳の入力・出力の行ごとの [棒, 数値] */
  private brainRows: [HTMLElement, HTMLElement][][] = [];
  /** 親のボタンを作り直さずに済むよう、前回の親の id を覚えておく */
  private lastParents: string | null = null;

  constructor(
    private readonly root: HTMLElement,
    handlers: InspectorHandlers,
  ) {
    root.innerHTML = `
      <div class="insp-head">
        <i class="dot" data-k="dot"></i>
        <span class="insp-title" data-k="title">–</span>
        <button type="button" data-k="follow" title="この個体をカメラで追う">追従</button>
        <button type="button" class="insp-close" data-k="close" aria-label="閉じる">×</button>
      </div>
      <div class="insp-death" data-k="death"></div>
      <div class="insp-note" data-k="reselect"></div>
      <div class="insp-body" data-k="body">
        <div class="insp-state" data-k="state">–</div>
        <div class="insp-hunger">
          <div class="insp-hunger-head"><span class="label">空腹度</span><span class="insp-hunger-value" data-k="hunger">–</span></div>
          <div class="meter"><div class="meter-fill" data-k="hungerBar"></div></div>
          <div class="insp-note" data-k="energy">–</div>
        </div>
        <dl class="hud-rows">
          <dt>消費</dt><dd data-k="cost">–</dd>
          <dt title="何も食べなかった場合。秒は等倍のとき">餓死まで</dt><dd data-k="starve">–</dd>
          <dt>年齢</dt><dd data-k="age">–</dd>
          <dt title="成長度が 100% になるまで繁殖できない。小さいうちは遅く、狩られやすい">成長</dt><dd data-k="growth">–</dd>
          <dt>世代</dt><dd data-k="generation">–</dd>
          <dt>子</dt><dd data-k="children">–</dd>
          <dt>速度</dt><dd data-k="speed">–</dd>
          <dt>位置</dt><dd data-k="pos">–</dd>
          <dt title="足元の草で、相手から見つかる距離が何割縮むか">隠れ</dt><dd data-k="hidden">–</dd>
          <dt title="草食は、生まれた場所と大きく違う植生（肥沃度）の草を食べられない">植生</dt><dd data-k="habitat">–</dd>
          <dt title="好みの気温から離れるほど、体温を保つための消費が増える（大きいほど割安）">気温</dt><dd data-k="temp">–</dd>
        </dl>
        <div class="insp-section">脳の判断（直前の tick）</div>
        <div class="priorities" data-k="priorities" title="どの欲求をどれだけ優先しているか。進む向きは、欲求ごとの方向をこの割合で混ぜて決まる。警戒（草食のみ）は高いほど天敵に遠くから気づけるが、一口が小さくなる"></div>
        <dl class="hud-rows">
          <dt>向き</dt><dd data-k="turn">–</dd>
          <dt>速さ</dt><dd data-k="throttle">–</dd>
          <dt>食べる</dt><dd data-k="wantEat">–</dd>
          <dt>繁殖</dt><dd data-k="wantChild">–</dd>
          <dt title="この距離以内に見えている同じ種類の個体がいないと繁殖できない">相手</dt><dd data-k="mate">–</dd>
        </dl>
        <div class="insp-section">遺伝子</div>
        <dl class="hud-rows">
          <dt>大きさ</dt><dd data-k="size">–</dd>
          <dt>最高速度</dt><dd data-k="maxSpeed">–</dd>
          <dt>視野</dt><dd data-k="vision">–</dd>
          <dt title="繁殖のとき、手持ちのエネルギーのうち子に渡す割合">子への投資</dt><dd data-k="invest">–</dd>
          <dt title="肉食は、自分の大きさに対してこの範囲の大きさの獲物だけを狙う">好みの獲物</dt><dd data-k="preyRange">–</dd>
          <dt title="生存に影響しない遺伝子。系統の見分けに使う">標識色</dt><dd><i class="dot" data-k="hue"></i> <button type="button" class="link-button" data-k="lineage">この系統を見る</button></dd>
        </dl>
        <div class="insp-section">生涯の記録</div>
        <dl class="hud-rows">
          <dt title="草・獲物・死体から身についたエネルギーの合計">食べた量</dt><dd data-k="eaten">–</dd>
          <dt>仕留めた数</dt><dd data-k="kills">–</dd>
          <dt>移動距離</dt><dd data-k="distance">–</dd>
          <dt title="繁殖相手がいて交配を試みた回数。失敗は遺伝的に遠い相手との交配">交配</dt><dd data-k="matings">–</dd>
        </dl>
        <div class="insp-section">家族</div>
        <dl class="hud-rows">
          <dt>親</dt><dd data-k="parents">–</dd>
          <dt>生きている子</dt><dd data-k="liveKids">–</dd>
        </dl>
        <div class="insp-section">エネルギーの推移（直近 300 tick）</div>
        <canvas class="insp-energy" data-k="energyChart" aria-label="エネルギーの推移。点線は繁殖に必要な量"></canvas>
        <details class="insp-brain">
          <summary>脳の入力と出力（直前の tick）</summary>
          <p class="legend-note">棒は 0 を中心に、右が正、左が負。−1〜1 で切り詰めて描く（数値は実際の値）。出力は判断に使う前の値</p>
          <div class="brain-group">入力</div>
          <div class="brain-rows" data-k="brainIn"></div>
          <div class="brain-group">出力</div>
          <div class="brain-rows" data-k="brainOut"></div>
        </details>
      </div>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) {
      this.el[node.dataset.k!] = node;
    }
    this.el.close.addEventListener('click', handlers.onClose);
    this.el.follow.addEventListener('click', handlers.onToggleFollow);
    this.el.lineage.addEventListener('click', handlers.onShowLineage);
    // 親の id のボタン（中身は update で作り直す）
    this.el.parents.addEventListener('click', (e) => {
      const id = Number((e.target as HTMLElement).dataset.id);
      if (id > 0) handlers.onSelectId(id);
    });
    // 脳の入力・出力の行は、名前が変わらないので最初に作っておく
    this.brainRows = [this.makeBrainRows(this.el.brainIn, INPUT_LABELS), this.makeBrainRows(this.el.brainOut, OUTPUT_LABELS)];
    this.hide();
  }

  hide(): void {
    this.root.hidden = true;
  }

  /** 死んだ個体の代わりを自動で選ぶ予定などのお知らせ（空文字で消す） */
  setNote(text: string): void {
    this.el.reselect.textContent = text;
  }

  setFollow(on: boolean): void {
    this.el.follow.classList.toggle('active', on);
  }

  private makeBrainRows(root: HTMLElement, labels: string[]): [HTMLElement, HTMLElement][] {
    return labels.map((label) => {
      const row = document.createElement('div');
      row.className = 'brain-row';
      row.innerHTML = '<span class="brain-label"></span><div class="brain-bar"><div class="brain-fill"></div></div><span class="brain-value">–</span>';
      row.querySelector('.brain-label')!.textContent = label;
      root.append(row);
      return [row.querySelector<HTMLElement>('.brain-fill')!, row.querySelector<HTMLElement>('.brain-value')!];
    });
  }

  /** 脳の入力と出力、エネルギーの推移を表示する */
  updateExtra(brain: { input: number[]; output: number[] } | null, history: number[] | null, threshold: number): void {
    if (brain) {
      [brain.input, brain.output].forEach((values, g) => {
        this.brainRows[g].forEach(([fill, value], k) => {
          const x = values[k] ?? 0;
          const c = Math.max(-1, Math.min(1, x));
          // 0 を中心に、正なら右へ、負なら左へ伸ばす
          fill.style.left = `${50 + Math.min(0, c) * 50}%`;
          fill.style.width = `${Math.abs(c) * 50}%`;
          value.textContent = x.toFixed(2);
        });
      });
    }
    if (history) this.drawEnergy(history, threshold);
  }

  private drawEnergy(values: number[], threshold: number): void {
    const canvas = this.el.energyChart as HTMLCanvasElement;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    // 縦軸は 0 から（繁殖に必要な量の 2 倍か、それを超えたら最大値）まで
    const max = Math.max(threshold * 2, ...values, 1e-6);
    const y = (v: number) => h - 1 - (v / max) * (h - 2);
    // 繁殖に必要な量の点線
    ctx.strokeStyle = COLORS.marker;
    ctx.globalAlpha = 0.5;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(0, y(threshold));
    ctx.lineTo(w, y(threshold));
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    if (values.length < 2) return;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    values.forEach((v, i) => {
      // 右端が今。履歴が短いうちは右に寄せる
      const x = w - ((values.length - 1 - i) / (ENERGY_HISTORY - 1)) * w;
      if (i === 0) ctx.moveTo(x, y(v));
      else ctx.lineTo(x, y(v));
    });
    ctx.stroke();
  }

  update(v: number[]): void {
    this.root.hidden = false;
    const dead = v[INSPECT.status] === 2;
    this.root.classList.toggle('dead', dead);
    this.el.follow.toggleAttribute('disabled', dead);

    if (dead) {
      const cause = DEATH_LABEL[v[INSPECT.deathCause]] ?? '不明';
      this.el.death.textContent = `死亡: ${cause}（tick ${fmtInt.format(v[INSPECT.deathTick])}）`;
      return; // 他の値は死ぬ直前のまま残す
    }
    this.el.death.textContent = '';

    const kind = v[INSPECT.kind];
    const herb = kind === HERBIVORE;
    this.el.dot.style.background = herb ? COLORS.herbivore : COLORS.carnivore;
    this.el.dot.className = herb ? 'dot' : 'tri';
    this.el.title.textContent = `${herb ? '草食' : '肉食'} #${fmtInt.format(v[INSPECT.id])}`;
    this.el.state.textContent = STATE_LABEL[kind]?.[v[INSPECT.state]] ?? '–';

    // 繁殖に必要な量に対して、どれだけ足りないか。0% で満腹（すぐ繁殖する）
    const energy = v[INSPECT.energy];
    const threshold = v[INSPECT.reproThreshold];
    const hunger = Math.min(1, Math.max(0, 1 - energy / threshold));
    this.el.hunger.textContent = `${Math.round(hunger * 100)}%`;
    this.el.hungerBar.style.width = `${hunger * 100}%`;
    this.el.energy.textContent = `エネルギー ${energy.toFixed(2)} / 繁殖に必要な量 ${threshold.toFixed(1)}`;

    const cost = v[INSPECT.cost];
    this.el.cost.textContent = `${cost.toFixed(4)} / tick`;
    if (cost > 0) {
      const ticks = energy / cost;
      this.el.starve.textContent = `約 ${fmtInt.format(Math.round(ticks))} tick（${Math.round(ticks / TICK_HZ)} 秒）`;
    } else {
      this.el.starve.textContent = '–';
    }
    this.el.age.textContent = `${fmtInt.format(v[INSPECT.age])} / 寿命 ${fmtInt.format(v[INSPECT.maxAge])} tick`;
    this.el.generation.textContent = `第 ${fmtInt.format(v[INSPECT.generation])} 世代`;
    this.el.children.textContent = `${fmtInt.format(v[INSPECT.children])} 体`;
    this.el.speed.textContent = `${v[INSPECT.speed].toFixed(2)} px/tick`;
    this.el.pos.textContent = `${Math.round(v[INSPECT.x])}, ${Math.round(v[INSPECT.y])}`;
    this.el.habitat.textContent = herb
      ? `生まれ ${v[INSPECT.natal].toFixed(2)}・食べやすさ ${Math.round(v[INSPECT.habitatFit] * 100)}%`
      : '–';
    this.el.temp.textContent = `ここ ${v[INSPECT.temperature].toFixed(1)}°・好み ${v[INSPECT.tempPref].toFixed(1)}°`;
    this.el.hidden.textContent = `見つかる距離 −${Math.round(v[INSPECT.hidden] * 100)}%`;

    const pri: [string, number][] = [
      ['食べる', v[INSPECT.priFood]],
      ['繁殖', v[INSPECT.priMate]],
      ['逃げる', v[INSPECT.priFlee]],
      ['仲間', v[INSPECT.priKin]],
      ['自由', v[INSPECT.priFree]],
    ];
    if (herb) pri.push(['警戒', v[INSPECT.vigilance]]);
    pri.push(['シグナル', v[INSPECT.signal]]);
    this.el.priorities.innerHTML = pri
      .map(
        ([label, p]) =>
          `<div class="pri-row"><span>${label}</span><div class="meter"><div class="meter-fill" style="width:${Math.round(p * 100)}%"></div></div><span class="pri-value">${Math.round(p * 100)}%</span></div>`,
      )
      .join('');

    const turnDeg = (v[INSPECT.turn] * 180) / Math.PI;
    this.el.turn.textContent =
      Math.abs(turnDeg) < 1 ? 'まっすぐ' : `${turnDeg > 0 ? '右' : '左'}へ ${Math.abs(turnDeg).toFixed(0)}°`;
    this.el.throttle.textContent = `最高速度の ${Math.round(v[INSPECT.throttle] * 100)}%`;
    this.el.wantEat.textContent = v[INSPECT.wantEat] > 0 ? '食べたい' : '食べない';
    const rest = v[INSPECT.rest];
    this.el.wantChild.textContent =
      rest > 0 ? `休み中（あと ${fmtInt.format(rest)} tick）` : v[INSPECT.wantChild] > 0 ? 'したい' : 'しない';
    const compat = v[INSPECT.mateCompat];
    this.el.mate.textContent =
      v[INSPECT.mateNearby] > 0 ? `近くにいる（相性 ${Math.round(Math.max(0, compat) * 100)}%）` : '近くにいない';

    const growth = v[INSPECT.growth];
    this.el.growth.textContent =
      growth >= 1 ? '成体' : `幼体 ${Math.round(growth * 100)}%（大きさ ${(v[INSPECT.size] * growth).toFixed(2)}）`;
    this.el.size.textContent = `${v[INSPECT.size].toFixed(2)}（成体時）`;
    const vigor = v[INSPECT.vigor];
    this.el.maxSpeed.textContent =
      vigor < 1
        ? `${v[INSPECT.maxSpeed].toFixed(2)} px/tick（弱って ${Math.round(vigor * 100)}% しか出ない）`
        : `${v[INSPECT.maxSpeed].toFixed(2)} px/tick`;
    this.el.vision.textContent = `${v[INSPECT.vision].toFixed(1)} px・視野角 ${Math.round((v[INSPECT.fov] * 180) / Math.PI)}°`;
    this.el.invest.textContent = `${Math.round(v[INSPECT.invest] * 100)}%`;
    this.el.hue.style.background = HUE_PALETTE[hueIndex(v[INSPECT.hue])];
    this.el.preyRange.textContent = herb ? '–' : `自分の ${v[INSPECT.preyMin].toFixed(2)}〜${v[INSPECT.preyMax].toFixed(2)} 倍`;

    this.el.eaten.textContent = v[INSPECT.eaten].toFixed(1);
    this.el.kills.textContent = herb ? '–' : `${fmtInt.format(v[INSPECT.kills])} 体`;
    this.el.distance.textContent = `${fmtInt.format(Math.round(v[INSPECT.distance]))} px`;
    const matings = v[INSPECT.matings];
    this.el.matings.textContent =
      matings > 0 ? `${fmtInt.format(matings)} 回（うち失敗 ${fmtInt.format(v[INSPECT.failedMatings])} 回）` : '0 回';
    const parents = [v[INSPECT.parentA], v[INSPECT.parentB]].filter((id) => id > 0);
    const key = parents.join(',');
    if (key !== this.lastParents) {
      this.lastParents = key;
      this.el.parents.textContent = parents.length === 0 ? '初期個体' : '';
      for (const id of parents) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'link-button';
        b.dataset.id = String(id);
        b.title = '生きていれば、この親を選ぶ';
        b.textContent = `#${id}`;
        this.el.parents.append(b);
      }
    }
    this.el.liveKids.textContent = `${fmtInt.format(v[INSPECT.livingChildren])} 体`;
  }
}

/** 画面下に短いお知らせを出す */
export class Toast {
  private timer = 0;
  constructor(private readonly root: HTMLElement) {
    root.hidden = true;
  }

  show(message: string): void {
    this.root.textContent = message;
    this.root.hidden = false;
    clearTimeout(this.timer);
    this.timer = window.setTimeout(() => (this.root.hidden = true), 2000);
  }
}

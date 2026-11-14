// 選択中の個体の詳細パネル。

import { COLORS, TICK_HZ } from '../config';
import { CARNIVORE, DEATH, HERBIVORE, INSPECT, STATE } from '../protocol';
import { HUE_PALETTE, hueIndex } from '../render/palette';

export interface InspectorHandlers {
  onClose(): void;
  onToggleFollow(): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

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
  },
};

const DEATH_LABEL: Record<number, string> = {
  [DEATH.STARVED]: '餓死',
  [DEATH.OLD_AGE]: '寿命',
  [DEATH.EATEN]: '肉食に食べられた',
  [DEATH.INJURED]: '獲物の反撃を受けた',
};

export class Inspector {
  private readonly el: Record<string, HTMLElement> = {};

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
          <dt title="生存に影響しない遺伝子。系統の見分けに使う">標識色</dt><dd><i class="dot" data-k="hue"></i></dd>
        </dl>
      </div>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) {
      this.el[node.dataset.k!] = node;
    }
    this.el.close.addEventListener('click', handlers.onClose);
    this.el.follow.addEventListener('click', handlers.onToggleFollow);
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
    this.el.hidden.textContent = `見つかる距離 −${Math.round(v[INSPECT.hidden] * 100)}%`;

    const pri: [string, number][] = [
      ['食べる', v[INSPECT.priFood]],
      ['繁殖', v[INSPECT.priMate]],
      ['逃げる', v[INSPECT.priFlee]],
      ['仲間', v[INSPECT.priKin]],
      ['自由', v[INSPECT.priFree]],
    ];
    if (herb) pri.push(['警戒', v[INSPECT.vigilance]]);
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
    this.el.wantChild.textContent = v[INSPECT.wantChild] > 0 ? 'したい' : 'しない';
    const compat = v[INSPECT.mateCompat];
    this.el.mate.textContent =
      v[INSPECT.mateNearby] > 0 ? `近くにいる（相性 ${Math.round(Math.max(0, compat) * 100)}%）` : '近くにいない';

    const growth = v[INSPECT.growth];
    this.el.growth.textContent =
      growth >= 1 ? '成体' : `幼体 ${Math.round(growth * 100)}%（大きさ ${(v[INSPECT.size] * growth).toFixed(2)}）`;
    this.el.size.textContent = `${v[INSPECT.size].toFixed(2)}（成体時）`;
    this.el.maxSpeed.textContent = `${v[INSPECT.maxSpeed].toFixed(2)} px/tick`;
    this.el.vision.textContent = `${v[INSPECT.vision].toFixed(1)} px`;
    this.el.invest.textContent = `${Math.round(v[INSPECT.invest] * 100)}%`;
    this.el.hue.style.background = HUE_PALETTE[hueIndex(v[INSPECT.hue])];
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

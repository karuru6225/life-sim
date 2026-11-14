// 選択中の個体の詳細パネル。

import { COLORS, TICK_HZ } from '../config';
import { CARNIVORE, DEATH, HERBIVORE, INSPECT, STATE } from '../protocol';

export interface InspectorHandlers {
  onClose(): void;
  onToggleFollow(): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

const STATE_LABEL: Record<number, Record<number, string>> = {
  [HERBIVORE]: {
    [STATE.GRAZE]: 'その場で草を食べている',
    [STATE.SEEK]: '草の多い方へ移動中',
    [STATE.FLEE]: '肉食から逃げている',
    [STATE.WANDER]: '歩き回っている',
  },
  [CARNIVORE]: {
    [STATE.WANDER]: '獲物を探している',
    [STATE.CHASE]: '獲物を追っている',
    [STATE.EAT]: '獲物を食べた',
  },
};

const DEATH_LABEL: Record<number, string> = {
  [DEATH.STARVED]: '餓死',
  [DEATH.OLD_AGE]: '寿命',
  [DEATH.EATEN]: '肉食に食べられた',
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
          <dt>世代</dt><dd data-k="generation">–</dd>
          <dt>子</dt><dd data-k="children">–</dd>
          <dt>速度</dt><dd data-k="speed">–</dd>
          <dt>位置</dt><dd data-k="pos">–</dd>
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

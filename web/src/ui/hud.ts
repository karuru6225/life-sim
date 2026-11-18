// 観察パネル。個体数の推移と形質の平均（＝進化の進み具合）が主役。性能の数値は脇に置く。

import { AUTO_SUNLIGHT, COLORS, HISTORY_LENGTH, SIM_PARAMS, SPEEDS, SUNLIGHT_RANGE, TICK_HZ, WORLD_SIZES, type WorldSize } from '../config';
import type { FromWorker, TraitMeans } from '../protocol';
import type { ColorMode } from '../render/agents';
import type { MapMode } from '../render/terrain';

/** 地図の上に重ねるものの表示・非表示 */
export interface Layers {
  terrain: boolean;
  plants: boolean;
  carrion: boolean;
  herbivores: boolean;
  carnivores: boolean;
  border: boolean;
  lineage: boolean;
  marker: boolean;
}

/** 表示の切り替えボタンの並びと名前 */
export /** 地図に描ける植物の層: [層の番号（-1 は合計）, 名前, 説明] */
const PLANT_LAYERS = [
  [-1, '合計', '全部の層の植物量'],
  [0, '地面の草', '地面の草の量。誰でも届く。大きい口ほど短い草を食べ残す'],
  [1, '茂み', '低い茂みの量。体長 0.85（大きさ約 0.6）から届き始める'],
  [2, '枝', '高い枝の量。体長 1.05（大きさ約 1.16）から届き始める'],
] as const;

const LAYER_LABELS: [keyof Layers, string, string][] = [
  ['terrain', '地形', '水域・陸の色（地図の表示方法で変わる）'],
  ['plants', '植物', '植物量'],
  ['carrion', '死体', '死体の量'],
  ['herbivores', '草食', '草食の個体'],
  ['carnivores', '肉食', '肉食の個体'],
  ['border', '枠', '世界の外枠'],
  ['lineage', '系統の印', '選んだ系統の重心と広がり'],
  ['marker', '選択の印', '選んだ個体の印と視野の円'],
];

/** 強調表示する種類（none は強調なし） */
export type Highlight = 'none' | 'herbivore' | 'carnivore';
/** 強調の方法: もう一方を薄くする / 強調する種類に枠線を付ける */
export type HighlightStyle = 'dim' | 'outline';
import { HUE_PALETTE } from '../render/palette';

type Stats = Extract<FromWorker, { type: 'stats' }>;

export interface HudHandlers {
  onTogglePause(): void;
  /** 統計のウインドウを開く・閉じる */
  onToggleStats(): void;
  /** 脳のウインドウを開く・閉じる */
  onToggleBrain(): void;
  onReset(): void;
  onSpeed(speed: number): void;
  onSunlight(value: number): void;
  onColorMode(mode: ColorMode): void;
  onAutoSunlight(enabled: boolean): void;
  onHighlight(target: Highlight): void;
  onHighlightStyle(style: HighlightStyle): void;
  onMapMode(mode: MapMode): void;
  /** 地図に描く植物の層（-1 は合計） */
  onPlantLayer(layer: number): void;
  /** 地図の上に重ねるものの表示を切り替える */
  onLayer(layer: keyof Layers): void;
  /** 世界の広さを選ぶ（世界を作り直す） */
  onWorldSize(size: WorldSize): void;
  /** 世界をファイルに保存する */
  onSave(): void;
  /** 保存したファイルから世界を読み込む */
  onLoad(file: File): void;
}

const fmtInt = new Intl.NumberFormat('ja-JP');

/** 形質の表の行: [ラベル, 値の取り出し方, 小数の桁数] */
const TRAIT_ROWS: [string, (m: TraitMeans) => number, number][] = [
  ['大きさ（成体時）', (m) => m.size, 2],
  ['最高速度', (m) => m.speed, 2],
  ['視野', (m) => m.vision, 1],
  ['子への投資', (m) => m.invest, 2],
  ['繁殖シグナル', (m) => m.signal, 2],
  ['世代', (m) => m.generation, 0],
  ['集まり具合', (m) => m.clustering, 2],
  ['向きのそろい', (m) => m.alignment, 2],
  ['　同じ系統どうし', (m) => m.alignSame, 2],
  ['　違う系統どうし', (m) => m.alignOther, 2],
  ['近くの同系統の割合', (m) => m.sameShare, 2],
];

/** 1 年のうちの位置（0〜1、0.25 で最も暖かい）を、季節の名前と、寒い地域の気温の平年との差で表す */
function seasonLabel(phase: number, season: number): string {
  if (SIM_PARAMS.season_length <= 0) return '季節なし';
  const names = ['春', '夏', '秋', '冬'];
  // 最も暖かい 0.25 を夏の真ん中にする
  const name = names[Math.floor(((phase + 0.125) % 1) * 4)];
  const parts = [name];
  const temp = Math.round(season * SIM_PARAMS.season_temp);
  if (SIM_PARAMS.season_temp !== 0) parts.push(`寒い地域 ${temp >= 0 ? '+' : '−'}${Math.abs(temp)}°`);
  const sun = Math.round(season * SIM_PARAMS.season_amplitude * 100);
  if (SIM_PARAMS.season_amplitude !== 0) parts.push(`日射 ${sun >= 0 ? '+' : '−'}${Math.abs(sun)}%`);
  return parts.join(' · ');
}

export class Hud {
  private readonly el: Record<string, HTMLElement> = {};
  private readonly speedButtons: HTMLButtonElement[] = [];
  private readonly modeButtons: HTMLButtonElement[] = [];
  private readonly highlightButtons: HTMLButtonElement[] = [];
  private readonly styleButtons: HTMLButtonElement[] = [];
  private readonly mapButtons: HTMLButtonElement[] = [];
  private readonly plantLayerButtons: HTMLButtonElement[] = [];
  private readonly layerButtons: HTMLButtonElement[] = [];
  private readonly sizeButtons: HTMLButtonElement[] = [];
  private readonly traitCells: HTMLElement[][] = [];
  private readonly chart: PopulationChart;

  constructor(root: HTMLElement, handlers: HudHandlers) {
    root.innerHTML = `
      <div class="hud-title">life-sim <span>phase 2 · 遺伝と進化</span></div>
      <div class="hud-primary">
        <div class="metric">
          <span class="value" data-k="herb">–</span>
          <span class="label"><i class="dot" style="background:${COLORS.herbivore}"></i>草食</span>
        </div>
        <div class="metric">
          <span class="value" data-k="carn">–</span>
          <span class="label"><i class="tri" style="background:${COLORS.carnivore}"></i>肉食</span>
        </div>
      </div>
      <canvas class="hud-chart" data-k="chart" aria-label="直近 2 分の個体数の推移"></canvas>
      <div class="chart-caption">直近 2 分 · 線ごとに縦軸の範囲が違う</div>
      <div class="lineage">
        <div class="lineage-head"><span class="group-label">系統色の分布（草食）</span><span class="lineage-max" data-k="linHerbMax">–</span></div>
        <canvas class="lineage-chart" data-k="linHerb" aria-label="草食の系統色ごとの個体数"></canvas>
        <div class="lineage-head"><span class="group-label">系統色の分布（肉食）</span><span class="lineage-max" data-k="linCarnMax">–</span></div>
        <canvas class="lineage-chart" data-k="linCarn" aria-label="肉食の系統色ごとの個体数"></canvas>
        <p class="legend-note">棒をクリックすると、その系統の統計を見られる</p>
        <div class="lineage-panel" data-k="linPanel"></div>
      </div>
      <table class="trait-table">
        <caption>形質の平均と群れの度合い</caption>
        <thead><tr><th></th><th>草食</th><th>肉食</th></tr></thead>
        <tbody data-k="traits"></tbody>
      </table>
      <dl class="hud-rows">
        <dt>植物量</dt><dd data-k="plant">–</dd>
        <dt title="寒い地域（上下の端）ほど季節で気温が大きく変わり、冬は草が育たず夏に育つ。暖かい中央はほぼ変わらない">季節</dt><dd data-k="season">–</dd>
        <dt title="繁殖相手と遺伝的に遠く、交配に失敗した割合。上がってきたら種が分かれつつある">交配失敗</dt><dd data-k="mateFail">–</dd>
        <dt title="肉食が届く距離の獲物に襲いかかったうち、仕留めた割合（直近 0.5 秒）。追いかけただけで襲えなかった分は数えない">狩りの成功率</dt><dd data-k="huntRate">–</dd>
        <dt>日射量</dt>
        <dd class="slider-row">
          <input type="range" data-k="sun" min="${SUNLIGHT_RANGE.min}" max="${SUNLIGHT_RANGE.max}" step="${SUNLIGHT_RANGE.step}" aria-label="日射量" />
          <span data-k="sunValue">–</span>
        </dd>
        <dt title="計算が重くなったら日射量を少しずつ下げて個体数を減らし、軽くなったら設定値まで戻す（最大 3 割まで）。既定はオフで、重いときは再生が遅くなる">自動調整</dt>
        <dd>
          <label class="check"><input type="checkbox" data-k="auto" ${AUTO_SUNLIGHT.enabledByDefault ? 'checked' : ''} /> 重いとき下げる</label>
          <div class="auto-state" data-k="autoState">–</div>
        </dd>
      </dl>
      <div class="hud-controls">
        <div class="group">
          <button type="button" data-k="pause">一時停止</button>
          <button type="button" data-k="reset">リセット</button>
          <button type="button" data-k="save" title="今の世界をファイルに保存する（乱数の状態も含むので、読み込むと同じ歴史が続く）">保存</button>
          <button type="button" data-k="load" title="保存したファイル（.lifesim.gz）から世界を読み込む">読み込み</button>
          <button type="button" data-k="stats" title="長期の統計（大きさの分布・個体数と形質・死因と出生）のウインドウを開く・閉じる（G キー）">統計</button>
          <button type="button" data-k="brain" title="選択中の個体の脳（すべてのニューロンとつながり、今の活動）のウインドウを開く・閉じる（B キー）">脳</button>
          <input type="file" data-k="loadFile" accept=".gz,.lifesim.gz,.lifesim" hidden />
        </div>
        <div class="group" data-k="sizes"><span class="group-label">世界の広さ（選ぶと作り直す）</span></div>
        <div class="group" data-k="speeds"><span class="group-label">速度</span></div>
        <div class="group" data-k="modes"><span class="group-label">色分け</span></div>
        <div class="group" data-k="highlights"><span class="group-label">強調（引いても見えるよう大きめに描く）</span></div>
        <div class="group" data-k="styles"><span class="group-label">強調の方法</span></div>
        <div class="group" data-k="maps"><span class="group-label">地図の表示方法</span></div>
        <div class="group" data-k="plantLayers"><span class="group-label">植物の表示（高さの層）</span></div>
        <div class="group" data-k="layers"><span class="group-label">表示するもの</span></div>
      </div>
      <dl class="hud-rows hud-perf">
        <dt>描画</dt><dd><span data-k="fps">–</span> fps</dd>
        <dt>計算</dt><dd><span data-k="ms">–</span> ms/tick</dd>
        <dt>速度</dt><dd><span data-k="tps">–</span> tick/s</dd>
        <dt>余裕</dt><dd data-k="budget">–</dd>
        <dt>tick</dt><dd data-k="tick">–</dd>
      </dl>
      <div class="hud-legend">
        <div data-k="lineageLegend" hidden>
          <span class="group-label">系統色（標識色の遺伝子）</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${HUE_PALETTE.join(', ')})"></div>
          <p class="legend-note">生存に影響しない遺伝子で、親から子へ少しずつずれながら受け継がれる。近い色ほど近い系統</p>
        </div>
        <div data-k="tempLegend" hidden>
          <span class="group-label" data-k="tempLegendLabel">平均気温</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.tempCold}, ${COLORS.tempMild}, ${COLORS.tempHot})"></div>
          <div class="swatch-scale"><span data-k="tempMin">–</span><span data-k="tempMax">–</span></div>
        </div>
        <div data-k="elevLegend" hidden>
          <span class="group-label">標高（高い所ほど寒い。季節で変わらない）</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.deep}, ${COLORS.shallow} 49%, ${COLORS.lowland} 51%, ${COLORS.highland})"></div>
          <div class="swatch-scale"><span>深い水</span><span>水際</span><span>最も高い所</span></div>
        </div>
        <div data-k="woodLegend" hidden>
          <span class="group-label">林と草原（植物の層の割合。土の肥えた場所ほど林になる。季節で変わらない）</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.grassland}, ${COLORS.woodland})"></div>
          <div class="swatch-scale"><span>草原（地面の草が多い）</span><span>林（茂みと枝が多い）</span></div>
        </div>
        <div data-k="growthLegend" hidden>
          <span class="group-label">今の育ちやすさ（土の肥えかた × 今の気温 × 雨の帯。今の最大を右端とする）</span>
          <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.landPoor}, ${COLORS.growthHigh})"></div>
          <div class="swatch-scale"><span>育たない</span><span>最も育つ</span></div>
        </div>
        <div data-k="vegLegend">
          <span class="group-label">地形</span>
          <div class="terrain-legend">
            <span><i class="swatch" style="background:${COLORS.water}"></i>水域（入れない）</span>
            <span><i class="swatch" style="background:linear-gradient(to right, ${COLORS.landPoor}, ${COLORS.landRich})"></i>陸（明るいほど肥沃）</span>
          </div>
        </div>
        <span class="group-label" data-k="plantLegendLabel">植物量（全部の層の合計）</span>
        <div class="plant-bar" style="background:linear-gradient(to right, ${COLORS.background}, ${COLORS.plant})"></div>
        <div class="swatch-scale"><span>なし</span><span>上限</span></div>
        <div class="terrain-legend">
          <span><i class="swatch" style="background:${COLORS.carrion}"></i>死体（肉食が食べられる。少しずつ腐って減る）</span>
        </div>
        <p class="legend-note">草食は丸、肉食は進行方向を指す三角。大きさは個体のいまの大きさに比例（肉食は見やすさのため 1.5 倍で表示）</p>
        <p class="legend-note">集まり具合: 最寄りの仲間までの距離 ÷ ランダムに散らばった場合の期待値（1 でランダム、小さいほど集まっている）。向きのそろい: 近くの仲間との向きの一致度（0 でばらばら、1 で同じ向き）。同じ系統どうしだけ高ければ、系統ごとの群れができている</p>
      </div>
      <p class="hud-help">ドラッグ: 移動 · ホイール: ズーム · ダブルクリック: 全体 · 拡大してクリック: 個体を選択 · Space: 一時停止</p>
    `;
    for (const node of root.querySelectorAll<HTMLElement>('[data-k]')) {
      this.el[node.dataset.k!] = node;
    }

    this.el.pause.addEventListener('click', handlers.onTogglePause);
    this.el.reset.addEventListener('click', handlers.onReset);
    this.el.save.addEventListener('click', handlers.onSave);
    this.el.stats.addEventListener('click', handlers.onToggleStats);
    this.el.brain.addEventListener('click', handlers.onToggleBrain);
    const loadFile = this.el.loadFile as HTMLInputElement;
    this.el.load.addEventListener('click', () => loadFile.click());
    loadFile.addEventListener('change', () => {
      const file = loadFile.files?.[0];
      if (file) handlers.onLoad(file);
      loadFile.value = ''; // 同じファイルをもう一度選べるように
    });
    const auto = this.el.auto as HTMLInputElement;
    auto.addEventListener('change', () => handlers.onAutoSunlight(auto.checked));
    const sun = this.el.sun as HTMLInputElement;
    sun.addEventListener('input', () => {
      this.el.sunValue.textContent = Number(sun.value).toFixed(3);
      handlers.onSunlight(Number(sun.value));
    });

    for (const speed of SPEEDS) {
      const b = button(speed === Infinity ? '最大' : `${speed}×`, () => handlers.onSpeed(speed));
      b.dataset.value = String(speed);
      this.speedButtons.push(b);
      this.el.speeds.append(b);
    }
    for (const [mode, label] of [
      ['kind', '種類'],
      ['lineage', '系統'],
    ] as const) {
      const b = button(label, () => handlers.onColorMode(mode));
      b.dataset.value = mode;
      this.modeButtons.push(b);
      this.el.modes.append(b);
    }

    for (const [target, label] of [
      ['none', 'なし'],
      ['herbivore', '草食'],
      ['carnivore', '肉食'],
    ] as const) {
      const b = button(label, () => handlers.onHighlight(target));
      b.dataset.value = target;
      this.highlightButtons.push(b);
      this.el.highlights.append(b);
    }

    for (const [style, label] of [
      ['dim', 'もう一方を薄く'],
      ['outline', '枠線'],
    ] as const) {
      const b = button(label, () => handlers.onHighlightStyle(style));
      b.dataset.value = style;
      this.styleButtons.push(b);
      this.el.styles.append(b);
    }

    for (const [mode, label, title] of [
      ['vegetation', '植生', '植生の種類（土の肥えかた）。季節で変わらない'],
      ['elevation', '標高', '陸の高さと水の深さ。高い所ほど寒い。季節で変わらない'],
      ['woodland', '林と草原', '植物の層の割合。草原は地面の草が多く、林は茂みと枝が多い。季節で変わらない'],
      ['temperature', '平均気温', '1 年を通した平均の気温'],
      ['tempNow', '今の気温', '季節の変化を含めた今の気温。寒い地域ほど季節で大きく変わる'],
      ['growth', '育ちやすさ', '今の植物の育ちやすさ。冬は寒い地域で育たない。雨の帯が季節で東西に動く（夏は東へ、冬は西へ）'],
    ] as const) {
      const b = button(label, () => handlers.onMapMode(mode));
      b.dataset.value = mode;
      b.title = title;
      this.mapButtons.push(b);
      this.el.maps.append(b);
    }
    for (const [layer, label, title] of PLANT_LAYERS) {
      const b = button(label, () => handlers.onPlantLayer(layer));
      b.dataset.value = String(layer);
      b.title = title;
      this.plantLayerButtons.push(b);
      this.el.plantLayers.append(b);
    }
    for (const [size, def] of Object.entries(WORLD_SIZES) as [WorldSize, (typeof WORLD_SIZES)[WorldSize]][]) {
      const b = button(def.label, () => handlers.onWorldSize(size));
      b.dataset.value = size;
      b.title = `${def.width} × ${def.height}、初期は草食 ${def.herbivores.toLocaleString('ja-JP')}・肉食 ${def.carnivores.toLocaleString('ja-JP')}`;
      this.sizeButtons.push(b);
      this.el.sizes.append(b);
    }
    for (const [layer, label, title] of LAYER_LABELS) {
      const b = button(label, () => handlers.onLayer(layer));
      b.dataset.value = layer;
      b.title = `${title}の表示・非表示`;
      b.setAttribute('aria-pressed', 'true');
      this.layerButtons.push(b);
      this.el.layers.append(b);
    }

    for (const [label] of TRAIT_ROWS) {
      const tr = document.createElement('tr');
      const th = document.createElement('th');
      th.textContent = label;
      const cells = [document.createElement('td'), document.createElement('td')];
      cells.forEach((c) => (c.textContent = '–'));
      tr.append(th, ...cells);
      this.el.traits.append(tr);
      this.traitCells.push(cells);
    }

    this.chart = new PopulationChart(this.el.chart as HTMLCanvasElement);
  }

  /** 系統色のヒストグラムを描く場所（[草食, 肉食] のキャンバスと、最大値の表示） */
  /** 系統の統計パネルを置く場所 */
  lineagePanelRoot(): HTMLElement {
    return this.el.linPanel;
  }

  lineageTargets(): [[HTMLCanvasElement, HTMLCanvasElement], [HTMLElement, HTMLElement]] {
    return [
      [this.el.linHerb as HTMLCanvasElement, this.el.linCarn as HTMLCanvasElement],
      [this.el.linHerbMax, this.el.linCarnMax],
    ];
  }

  setFps(fps: number): void {
    this.el.fps.textContent = fps.toFixed(0);
  }

  setStats(s: Stats, speed: number, paused: boolean): void {
    this.el.herb.textContent = fmtInt.format(s.herbivore.count);
    this.el.carn.textContent = fmtInt.format(s.carnivore.count);
    this.el.plant.textContent = fmtInt.format(Math.round(s.plantTotal));
    this.el.season.textContent = seasonLabel(s.seasonPhase, s.season);
    this.el.huntRate.textContent = Number.isFinite(s.huntSuccessRate) ? `${(s.huntSuccessRate * 100).toFixed(1)}%` : '–';
    this.el.mateFail.textContent = Number.isFinite(s.matingFailRate) ? `${(s.matingFailRate * 100).toFixed(1)}%` : '–';
    this.el.autoState.textContent =
      s.sunlightFactor < 1
        ? `実効 ${s.sunlight.toFixed(3)}（設定値の −${((1 - s.sunlightFactor) * 100).toFixed(1)}%）`
        : '設定値のまま';

    TRAIT_ROWS.forEach(([, get, digits], r) => {
      [s.herbivore, s.carnivore].forEach((m, c) => {
        const value = get(m);
        this.traitCells[r][c].textContent = m.count === 0 ? '絶滅' : Number.isFinite(value) ? value.toFixed(digits) : '–';
      });
    });

    this.el.tps.textContent = s.ticksPerSecond.toFixed(1);
    this.el.ms.textContent = s.msPerTick > 0 ? s.msPerTick.toFixed(2) : '–';
    if (s.msPerTick > 0) {
      const maxTps = 1000 / s.msPerTick;
      this.el.budget.textContent = `${fmtInt.format(Math.round(maxTps))} tick/s まで（${(maxTps / TICK_HZ).toFixed(1)}×）`;
    }
    const lagging = !paused && speed !== Infinity && s.ticksPerSecond < TICK_HZ * speed * 0.9;
    this.el.tps.classList.toggle('warn', lagging);

    if (!paused) this.chart.push(s.herbivore.count, s.carnivore.count);
  }

  clearHistory(): void {
    this.chart.clear();
  }

  setTick(tick: number): void {
    this.el.tick.textContent = fmtInt.format(tick);
  }

  setSpeed(speed: number): void {
    for (const b of this.speedButtons) b.classList.toggle('active', Number(b.dataset.value) === speed);
  }

  setColorMode(mode: ColorMode): void {
    for (const b of this.modeButtons) b.classList.toggle('active', b.dataset.value === mode);
    this.el.lineageLegend.hidden = mode !== 'lineage';
  }

  setHighlight(target: Highlight, style: HighlightStyle): void {
    for (const b of this.highlightButtons) b.classList.toggle('active', b.dataset.value === target);
    for (const b of this.styleButtons) b.classList.toggle('active', b.dataset.value === style);
  }

  setMapMode(mode: MapMode): void {
    for (const b of this.mapButtons) b.classList.toggle('active', b.dataset.value === mode);
    this.el.tempLegend.hidden = mode !== 'temperature' && mode !== 'tempNow';
    this.el.tempLegendLabel.textContent = mode === 'tempNow' ? '今の気温（色の範囲は 1 年を通して同じ）' : '平均気温';
    this.el.growthLegend.hidden = mode !== 'growth';
    this.el.woodLegend.hidden = mode !== 'woodland';
    this.el.elevLegend.hidden = mode !== 'elevation';
    // 地形の凡例（肥沃度の明るさ）は植生の表示のときだけ正しい
    this.el.vegLegend.hidden = mode !== 'vegetation';
  }

  /** 地図に描いている植物の層（-1 は合計）をボタンと凡例に反映する */
  setPlantLayer(layer: number): void {
    for (const b of this.plantLayerButtons) b.classList.toggle('active', b.dataset.value === String(layer));
    const entry = PLANT_LAYERS.find(([k]) => k === layer);
    this.el.plantLegendLabel.textContent =
      layer < 0 || !entry ? '植物量（全部の層の合計）' : `植物量（${entry[1]}だけ。その層が溜まりうる最大の量で最も濃い）`;
  }

  /** 選んでいる世界の広さ（どれでもなければ null。読み込んだ世界が選択肢と違う大きさのとき） */
  setWorldSize(size: WorldSize | null): void {
    for (const b of this.sizeButtons) b.classList.toggle('active', b.dataset.value === size);
  }

  /** 表示するものの切り替えボタンの状態をそろえる（押されている = 表示中） */
  setLayers(layers: Layers): void {
    for (const b of this.layerButtons) {
      const on = layers[b.dataset.value as keyof Layers];
      b.classList.toggle('active', on);
      b.setAttribute('aria-pressed', String(on));
    }
  }

  setTempRange([min, max]: [number, number]): void {
    this.el.tempMin.textContent = `${min.toFixed(0)}°`;
    this.el.tempMax.textContent = `${max.toFixed(0)}°`;
  }

  /** 統計のウインドウが開いているかを、ボタンの見た目に反映する */
  setStatsOpen(open: boolean): void {
    this.el.stats.classList.toggle('active', open);
    this.el.stats.setAttribute('aria-pressed', String(open));
  }

  /** 脳のウインドウが開いているかを、ボタンの見た目に反映する */
  setBrainOpen(open: boolean): void {
    this.el.brain.classList.toggle('active', open);
    this.el.brain.setAttribute('aria-pressed', String(open));
  }

  setPaused(paused: boolean): void {
    this.el.pause.textContent = paused ? '再開' : '一時停止';
    this.el.pause.classList.toggle('active', paused);
  }

  setSunlight(value: number): void {
    (this.el.sun as HTMLInputElement).value = String(value);
    this.el.sunValue.textContent = value.toFixed(3);
  }
}

function button(label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = label;
  b.addEventListener('click', onClick);
  return b;
}

/**
 * 草食と肉食の個体数の推移。数が 1 桁違うので、線ごとに自分の最小〜最大で縦軸を取る
 * （絶対値は上の数字が担当し、グラフは増減の形を見せる）。
 */
class PopulationChart {
  private herb: number[] = [];
  private carn: number[] = [];
  private readonly ctx: CanvasRenderingContext2D;

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext('2d')!;
  }

  push(herb: number, carn: number): void {
    this.herb.push(herb);
    this.carn.push(carn);
    if (this.herb.length > HISTORY_LENGTH) {
      this.herb.shift();
      this.carn.shift();
    }
    this.draw();
  }

  clear(): void {
    this.herb = [];
    this.carn = [];
    this.draw();
  }

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (this.canvas.width !== Math.round(w * dpr)) {
      this.canvas.width = Math.round(w * dpr);
      this.canvas.height = Math.round(h * dpr);
    }
    const ctx = this.ctx;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    this.line(this.herb, COLORS.herbivore, w, h);
    this.line(this.carn, COLORS.carnivore, w, h);
  }

  private line(values: number[], color: string, w: number, h: number): void {
    if (values.length < 2) return;
    let min = Math.min(...values);
    let max = Math.max(...values);
    // 変動が小さいときに、わずかな揺れが画面いっぱいの山に化けないよう、最低限の幅を持たせる
    const minSpan = Math.max(1, max * 0.2);
    if (max - min < minSpan) {
      const mid = (max + min) / 2;
      min = mid - minSpan / 2;
      max = mid + minSpan / 2;
    }
    const pad = 3;
    const ctx = this.ctx;
    ctx.beginPath();
    values.forEach((v, i) => {
      const x = (i / (HISTORY_LENGTH - 1)) * w;
      const y = pad + (1 - (v - min) / (max - min)) * (h - pad * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }
}

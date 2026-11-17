// メインスレッドと Worker の間で交わすメッセージの型

export type ToWorker =
  /** params の値をすべて設定してから世界を作り直す */
  | { type: 'init'; seed: number; params: Record<string, number> }
  /** 実行中にパラメータを変える（世界の大きさなどはリセットまで効かない） */
  | { type: 'setParam'; name: string; value: number }
  /** Infinity は「処理できる限り速く」 */
  | { type: 'setSpeed'; speed: number }
  | { type: 'setPaused'; paused: boolean }
  /** 日射量の自動調整のオン・オフ */
  | { type: 'setAutoSunlight'; enabled: boolean }
  /**
   * 世界座標 (x, y) から radius 以内で最も近い個体を選択する。
   * kind を指定するとその種類を優先し、いなければ種類を問わず選ぶ
   */
  | { type: 'select'; x: number; y: number; radius: number; kind?: number }
  | { type: 'deselect' }
  /** 系統の中からランダムに 1 体を選択する。結果は selected で返る */
  | { type: 'selectRandomInLineage'; lineage: LineageSelection }
  /** id の個体を選択する（親や子をたどるとき）。結果は selected で返る */
  | { type: 'selectId'; id: number }
  /** 統計を取る系統を選ぶ（null で解除） */
  | { type: 'setLineage'; lineage: LineageSelection | null }
  /** 地図に描く植物の層（0: 地面の草、1: 茂み、2: 枝、-1: 合計）。フレームの植物量がその層になる */
  | { type: 'setPlantLayer'; layer: number }
  /** 世界を保存する。結果は saved で返る */
  | { type: 'save' }
  /** 保存したファイル（gzip のまま）から世界を読み込む。結果は loaded で返る（成功なら、その前に ready も届く） */
  | { type: 'load'; buffer: ArrayBuffer }
  /** 描画し終えたバッファを Worker に返して再利用させる */
  | { type: 'returnBuffer'; buffer: ArrayBuffer };

export type FromWorker =
  | {
      type: 'ready';
      generation: number;
      /** 地形（Float32、cols × rows、行優先）。水は −1、陸は肥沃度 0〜1。世界を作り直すまで変わらない */
      terrain: ArrayBuffer;
      /** 平均気温（Float32、cols × rows、度） */
      temperature: ArrayBuffer;
      /** 林の度合い（Float32、cols × rows）。水は −1、陸は 0（最も草原）〜1（最も林）。世界を作り直すまで変わらない */
      wood: ArrayBuffer;
      /** 標高（Float32、cols × rows）。陸は 0（海面）〜1（最も高い所）、水は 0（水際）〜−1（最も深い所） */
      elevation: ArrayBuffer;
      terrainCols: number;
      terrainRows: number;
    }
  /** 保存したファイルの中身（gzip）とファイル名 */
  | { type: 'saved'; buffer: ArrayBuffer; filename: string; bytes: number }
  /**
   * 読み込みの結果。ok なら message は注意（別の版で保存された、など。なければ空）、失敗なら理由。
   * params は読み込んだ世界のパラメータ（成功時のみ）
   */
  | { type: 'loaded'; ok: boolean; message: string; params: Record<string, number> | null; tick: number }
  /** 季節で変わる地図のデータ（今の気温と、今の植物の育ちやすさ。Float32、cols × rows）。約 1 秒ごとに届く */
  | { type: 'climate'; generation: number; temperature: ArrayBuffer; fertility: ArrayBuffer }
  /**
   * 長期の統計の記録（Rust の world/history.rs）。新しい標本を取るたびに届く。
   * reset なら記録の全部、そうでなければ from 番目以降（from 番目はまとめ途中で、前に送ったものと変わっている）。
   * data は Float64 で、記録 1 つが recordLen 個: [tick, 区間の長さ, 草食の値 × HISTORY_KIND_LEN, 肉食の値 × HISTORY_KIND_LEN]
   */
  | {
      type: 'history';
      generation: number;
      reset: boolean;
      from: number;
      recordLen: number;
      /** 大きさの分布の段階の数と範囲（下限, 上限） */
      bins: number;
      sizeRange: [number, number];
      data: ArrayBuffer;
    }
  /** select の結果。id が 0 なら近くに個体がいなかった */
  | { type: 'selected'; id: number }
  | {
      type: 'frame';
      generation: number;
      /**
       * Float32 で、先頭から
       *   個体: [x, y, 種類, 大きさ, 標識色, 向き（ラジアン、肉食のみ）] × agentCount（1 個体 stride 要素）
       *   植物: plantCols × plantRows（行優先）
       *   死体: plantCols × plantRows（行優先）
       */
      buffer: ArrayBuffer;
      agentCount: number;
      stride: number;
      plantCols: number;
      plantRows: number;
      tick: number;
      /** 選択中の個体の情報（INSPECT の並び）。選択していなければ null */
      inspect: number[] | null;
      /** 選択中の個体の脳の入力・出力・中間ニューロンの出力（直前の tick。中間は 1 層目のあとに 2 層目）。選択していなければ null */
      brain: { input: number[]; output: number[]; hidden: number[]; h1: number } | null;
      /**
       * 選択中の個体の脳の重み（sim/src/genome.rs の brain_part() の並び）。選択した個体が変わったときだけ入る。
       * 個体がいない（死んだ）ときは空の配列
       */
      brainNet?: number[];
      /** brainNet の個体の id */
      brainNetId?: number;
      /** 選択中の個体の直近のエネルギー（古い順）。選択していなければ null */
      energyHistory: number[] | null;
    }
  | {
      type: 'stats';
      ticksPerSecond: number;
      msPerTick: number;
      plantTotal: number;
      /** 実際に使っている日射量（自動調整の結果） */
      sunlight: number;
      /** 自動調整で設定値の何倍にしているか（1 なら下げていない） */
      sunlightFactor: number;
      /** 直前の集計区間で、交配が相性で失敗した割合（交配がなければ NaN） */
      matingFailRate: number;
      /** 直前の集計区間で、肉食の襲撃のうち仕留めた割合（襲撃がなければ NaN） */
      huntSuccessRate: number;
      /** 季節（−1〜1、日射が最も多い時期に 1）と、1 年のうちの位置（0〜1、0.25 で日射が最も多い） */
      season: number;
      seasonPhase: number;
      herbivore: TraitMeans;
      carnivore: TraitMeans;
      /** 選んだ系統の統計（LINEAGE の並びで、前半が系統、後半が種類全体）。選んでいなければ null */
      lineage: number[] | null;
    };

/** 系統: 種類 kind のうち、標識色（色相 0〜1）が from から幅 width の範囲にある個体（1 をまたいでもよい） */
export interface LineageSelection {
  kind: number;
  from: number;
  width: number;
}

/** 系統の統計の並び。sim/src/world.rs の LINEAGE_STATS と一致させること。種類全体の値は +LINEAGE_LEN の位置 */
export const LINEAGE = {
  count: 0,
  juvenile: 1,
  size: 2,
  speed: 3,
  vision: 4,
  fovDeg: 5,
  tempPref: 6,
  invest: 7,
  signal: 8,
  generation: 9,
  centerX: 10,
  centerY: 11,
  spread: 12,
  fed: 13,
  coldShare: 14,
  brain1: 15,
  brain2: 16,
  litter: 17,
  /** 直近 LINEAGE_DEATH_WINDOW tick の死因ごとの死亡数の先頭（死因 1〜7 が 18〜24。死因の番号は DEATH） */
  deaths: 18,
} as const;
export const LINEAGE_LEN = 25;
/** 長期の統計を記録する間隔（tick）。sim/src/world/history.rs の HISTORY_EVERY と一致させること */
export const HISTORY_EVERY = 200;

/** 記録の種類ごとの値の並び。sim/src/world/history.rs の field と一致させること（死因は DEATHS + 死因の番号、分布は SIZE_HIST + 段階） */
export const HISTORY_FIELD = {
  count: 0,
  size: 1,
  speed: 2,
  vision: 3,
  fovDeg: 4,
  tempPref: 5,
  invest: 6,
  litter: 7,
  generation: 8,
  brain: 9,
  births: 10,
  deaths: 11,
  sizeHist: 19,
} as const;

/** 長期の統計の記録の、種類ごとの値。形質は記録した瞬間の平均、出生と死亡は区間内の数 */
export interface HistoryKind {
  count: number;
  /** 大人の大きさの平均 */
  size: number;
  speed: number;
  vision: number;
  /** 視野角（度） */
  fov: number;
  tempPref: number;
  invest: number;
  litter: number;
  generation: number;
  /** 使っている中間ニューロンの数（1 層目 + 2 層目） */
  brain: number;
  births: number;
  /** 死因ごとの死亡数（添字は DEATH の番号。0 は使わない） */
  deaths: number[];
  /** 大人の大きさの分布（sizeRange を対数で等間隔に分けた段階ごとの個体数） */
  sizeHist: number[];
}

/** 長期の統計の記録 1 つ。span は前の記録からの tick 数 */
export interface HistorySample {
  tick: number;
  span: number;
  /** 大きさの分布の範囲（下限, 上限） */
  sizeRange: [number, number];
  herb: HistoryKind;
  carn: HistoryKind;
}

/** 保存ファイルの見出し（JSON）。ファイルは gzip で、中身は SAVE_MAGIC・見出しの長さ（u32）・見出し・世界のバイト列 */
export interface SaveHeader {
  /** 世界のバイト列の形式の番号（sim/src/world/save.rs の FORMAT_VERSION） */
  format: number;
  /** 保存したコードのコミット */
  commit: string;
  savedAt: string;
  tick: number;
  herbivores: number;
  carnivores: number;
}
export const SAVE_MAGIC = 'LIFESIM\n';

/** 系統ごとの死因を数える期間（sim/src/world.rs の DEATH_WINDOW） */
export const LINEAGE_DEATH_WINDOW = 1200;

/** 種類ごとの集計（形質は平均） */
export interface TraitMeans {
  count: number;
  size: number;
  speed: number;
  vision: number;
  invest: number;
  generation: number;
  /** 集まり具合（Clark-Evans 比）。1 ならランダム、1 未満なら集まっている */
  clustering: number;
  /** 近くの仲間との向きのそろい具合。0 ならばらばら、1 なら全員同じ向き */
  alignment: number;
  /** 向きのそろい具合を、同じ系統どうし・違う系統どうしに分けたもの */
  alignSame: number;
  alignOther: number;
  /** 近くの仲間のうち、同じ系統の割合 */
  sameShare: number;
  /** 繁殖シグナルの平均（0〜1） */
  signal: number;
}

/** inspect 配列の各要素の位置。sim/src/lib.rs の inspect() と一致させること */
export const INSPECT = {
  /** 0 = 選択なし、1 = 生存、2 = 死亡 */
  status: 0,
  id: 1,
  kind: 2,
  x: 3,
  y: 4,
  energy: 5,
  reproThreshold: 6,
  age: 7,
  maxAge: 8,
  state: 9,
  speed: 10,
  cost: 11,
  children: 12,
  generation: 13,
  deathCause: 14,
  deathTick: 15,
  vision: 16,
  size: 17,
  maxSpeed: 18,
  invest: 19,
  hue: 20,
  /** 直前に曲がった角度（ラジアン、正で右） */
  turn: 21,
  /** 直前の速さ（最高速度に対する割合） */
  throttle: 22,
  /** 正なら繁殖したい */
  wantChild: 23,
  /** 正なら食べたい */
  wantEat: 24,
  /** 足元の草で、相手の視野が何割縮むか */
  hidden: 25,
  energyCap: 26,
  /** 1 なら繁殖相手が近くにいる */
  mateNearby: 27,
  /** 成長度（1 で成熟）。size は大人の大きさで、いまの大きさは size × growth */
  growth: 28,
  /** 繁殖相手との相性（繁殖が成功する確率）。相手がいなければ負 */
  mateCompat: 29,
  /** 欲求ごとの優先度（0〜1）。食べる・繁殖・逃げる・仲間・自由 */
  priFood: 30,
  priMate: 31,
  priFlee: 32,
  priKin: 33,
  priFree: 34,
  /** 警戒の度合い（0〜1、草食のみ） */
  vigilance: 35,
  /** 生まれた場所の植生（肥沃度） */
  natal: 36,
  /** 今いる場所の草の食べやすさ（0〜1、植生の違いによる。肉食は常に 1） */
  habitatFit: 37,
  /** 今いる場所の平均気温と、好みの気温（度） */
  temperature: 38,
  tempPref: 39,
  /** 繁殖シグナルの強さ（0〜1） */
  signal: 40,
  /** 体の調子（飢えと老いで最高速度に掛かる倍率） */
  vigor: 41,
  /** 視野角（ラジアン） */
  fov: 42,
  /** 次に繁殖できるまでの残り tick 数 */
  rest: 43,
  /** 生涯の記録 */
  eaten: 44,
  kills: 45,
  distance: 46,
  matings: 47,
  failedMatings: 48,
  /** 親の id（繁殖を決めた側, 相手）。初期個体と無性生殖の相手は 0 */
  parentA: 49,
  parentB: 50,
  livingChildren: 51,
  /** 好みの獲物の大きさの比（肉食のみ） */
  preyMin: 52,
  preyMax: 53,
  /** 襲いかかった回数（肉食） */
  hunts: 54,
  /** 使っている中間ニューロンの数（1 層目, 2 層目） */
  brain1: 55,
  brain2: 56,
  /** 追い払う欲求の優先度（0〜1） */
  priFight: 57,
  /** 仲間（同じ種類）を最後に見てからの tick 数 */
  alone: 58,
  /** 一度に産む数 */
  litter: 59,
} as const;

export const HERBIVORE = 0;
export const CARNIVORE = 1;

/** sim/src/world.rs の state と一致させること */
export const STATE = { WANDER: 0, GRAZE: 1, SEEK: 2, FLEE: 3, CHASE: 4, EAT: 5, MISSED: 6, COURT: 7, SCAVENGE: 8, FIGHT: 9, FIGHT_KIN: 10 } as const;

/** sim/src/world.rs の death と一致させること */
export const DEATH = { STARVED: 1, OLD_AGE: 2, EATEN: 3, INJURED: 4, SICK: 5, FOUGHT: 6, KIN_FOUGHT: 7 } as const;

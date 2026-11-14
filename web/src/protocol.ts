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
  /** 描画し終えたバッファを Worker に返して再利用させる */
  | { type: 'returnBuffer'; buffer: ArrayBuffer };

export type FromWorker =
  | {
      type: 'ready';
      generation: number;
      /** 地形（Float32、cols × rows、行優先）。水は −1、陸は肥沃度 0〜1。世界を作り直すまで変わらない */
      terrain: ArrayBuffer;
      terrainCols: number;
      terrainRows: number;
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
       */
      buffer: ArrayBuffer;
      agentCount: number;
      stride: number;
      plantCols: number;
      plantRows: number;
      tick: number;
      /** 選択中の個体の情報（INSPECT の並び）。選択していなければ null */
      inspect: number[] | null;
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
      herbivore: TraitMeans;
      carnivore: TraitMeans;
    };

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
} as const;

export const HERBIVORE = 0;
export const CARNIVORE = 1;

/** sim/src/world.rs の state と一致させること */
export const STATE = { WANDER: 0, GRAZE: 1, SEEK: 2, FLEE: 3, CHASE: 4, EAT: 5, MISSED: 6, COURT: 7 } as const;

/** sim/src/world.rs の death と一致させること */
export const DEATH = { STARVED: 1, OLD_AGE: 2, EATEN: 3 } as const;

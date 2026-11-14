// メインスレッドと Worker の間で交わすメッセージの型

export type ToWorker =
  /** params の値をすべて設定してから世界を作り直す */
  | { type: 'init'; seed: number; params: Record<string, number> }
  /** 実行中にパラメータを変える（世界の大きさなどはリセットまで効かない） */
  | { type: 'setParam'; name: string; value: number }
  /** Infinity は「処理できる限り速く」 */
  | { type: 'setSpeed'; speed: number }
  | { type: 'setPaused'; paused: boolean }
  /** 世界座標 (x, y) から radius 以内で最も近い個体を選択する */
  | { type: 'select'; x: number; y: number; radius: number }
  | { type: 'deselect' }
  /** 描画し終えたバッファを Worker に返して再利用させる */
  | { type: 'returnBuffer'; buffer: ArrayBuffer };

export type FromWorker =
  | { type: 'ready'; generation: number }
  /** select の結果。id が 0 なら近くに個体がいなかった */
  | { type: 'selected'; id: number }
  | {
      type: 'frame';
      generation: number;
      /**
       * Float32 で、先頭から
       *   個体: [x, y, 種類] × agentCount（1 個体 stride 要素）
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
      herbivores: number;
      carnivores: number;
      plantTotal: number;
    };

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
} as const;

export const HERBIVORE = 0;
export const CARNIVORE = 1;

/** sim/src/world.rs の state と一致させること */
export const STATE = { WANDER: 0, GRAZE: 1, SEEK: 2, FLEE: 3, CHASE: 4, EAT: 5 } as const;

/** sim/src/world.rs の death と一致させること */
export const DEATH = { STARVED: 1, OLD_AGE: 2, EATEN: 3 } as const;

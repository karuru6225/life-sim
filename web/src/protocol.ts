// メインスレッドと Worker の間で交わすメッセージの型

export type ToWorker =
  | { type: 'init'; seed: number; count: number; width: number; height: number }
  /** Infinity は「処理できる限り速く」 */
  | { type: 'setSpeed'; speed: number }
  | { type: 'setPaused'; paused: boolean }
  /** 描画し終えたバッファを Worker に返して再利用させる */
  | { type: 'returnBuffer'; buffer: ArrayBuffer };

export type FromWorker =
  | { type: 'ready'; generation: number; count: number; stride: number }
  | {
      type: 'frame';
      generation: number;
      /** [x, y, 近傍数] × count の Float32 データ */
      buffer: ArrayBuffer;
      count: number;
      tick: number;
    }
  | { type: 'stats'; ticksPerSecond: number; msPerTick: number };

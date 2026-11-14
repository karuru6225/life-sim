// メインスレッドと Worker の間で交わすメッセージの型

export type ToWorker =
  /** params の値をすべて設定してから世界を作り直す */
  | { type: 'init'; seed: number; params: Record<string, number> }
  /** 実行中にパラメータを変える（世界の大きさなどはリセットまで効かない） */
  | { type: 'setParam'; name: string; value: number }
  /** Infinity は「処理できる限り速く」 */
  | { type: 'setSpeed'; speed: number }
  | { type: 'setPaused'; paused: boolean }
  /** 描画し終えたバッファを Worker に返して再利用させる */
  | { type: 'returnBuffer'; buffer: ArrayBuffer };

export type FromWorker =
  | { type: 'ready'; generation: number }
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
    }
  | {
      type: 'stats';
      ticksPerSecond: number;
      msPerTick: number;
      herbivores: number;
      carnivores: number;
      plantTotal: number;
    };

// 調整用の設定値。Rust を触らずに変えられる値はここに集める。

export const WORLD = {
  seed: 1,
  width: 4800,
  height: 3200,
};

/** 等倍のときの 1 秒あたりの tick 数 */
export const TICK_HZ = 30;

/** 倍速の選択肢。Infinity は「処理できる限り速く」 */
export const SPEEDS = [1, 2, 4, 8, Infinity] as const;

export const COUNT_OPTIONS = [10_000, 50_000, 100_000, 200_000] as const;
export const DEFAULT_COUNT = 50_000;

/**
 * 色。暗視野顕微鏡の見え方（暗い視野に、生物が光って浮かぶ）から引いている。
 * 近傍密度は、まばら = 冷たい青緑、密集 = 暖かい淡黄。
 */
export const COLORS = {
  background: '#06090b',
  densityStops: ['#3a8f9c', '#9fd8c4', '#ffe9a8'],
};

/** 密度の色分けの段階数（0, 1, …, DENSITY_LEVELS - 1 以上） */
export const DENSITY_LEVELS = 9;

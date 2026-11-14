// 調整用の設定値。Rust を触らずに変えられる値はここに集める。

/**
 * シミュレーションのパラメータ。名前は sim/src/params.rs と一致させること
 * （Rust が知らない名前はブラウザのコンソールに警告が出る）。
 * 各値の意味は params.rs のコメントを参照。
 */
export const SIM_PARAMS = {
  // 世界（リセット時のみ反映）
  world_width: 4800,
  world_height: 3200,
  plant_cell: 32,
  herb_initial: 20000,
  carn_initial: 1500,
  plant_initial_ratio: 0.5,

  // 植物
  sunlight: 0.08,
  plant_capacity: 10,

  // 共通
  move_cost: 0.004,
  birth_spread: 6,

  // 草食
  herb_vision: 30,
  herb_graze_speed: 0.4,
  herb_speed: 0.9,
  herb_flee_speed: 1.3,
  herb_bite: 0.12,
  herb_efficiency: 0.6,
  herb_metabolism: 0.01,
  herb_repro_threshold: 10,
  herb_birth_cost: 1,
  herb_max_age: 2400,
  herb_initial_energy: 5,

  // 肉食
  carn_vision: 48,
  carn_attack_range: 6,
  carn_wander_speed: 0.6,
  carn_chase_speed: 1.8,
  carn_efficiency: 0.5,
  carn_metabolism: 0.012,
  carn_repro_threshold: 16,
  carn_birth_cost: 2,
  carn_max_age: 3600,
  carn_initial_energy: 8,
};

export type SimParams = typeof SIM_PARAMS;

export const SEED = 1;

/** 日射量スライダーの範囲 */
export const SUNLIGHT_RANGE = { min: 0, max: 0.2, step: 0.005 };

/** 等倍のときの 1 秒あたりの tick 数 */
export const TICK_HZ = 30;

/** 倍速の選択肢。Infinity は「処理できる限り速く」 */
export const SPEEDS = [1, 2, 4, 8, Infinity] as const;

/** 個体数グラフに残すサンプル数（stats は 0.5 秒ごと → 2 分ぶん） */
export const HISTORY_LENGTH = 240;

/**
 * 色。暗視野顕微鏡の見え方（暗い視野に、生物が光って浮かぶ）から引いている。
 * 植物は視野の地に沈む暗い緑、草食は淡い青緑、肉食は暖色。
 *
 * コントラスト比（WCAG、図形は 3:1 以上が目安）:
 *   植物満杯 / 地色 3.12
 *   草食 / 植物満杯 3.99、肉食 / 植物満杯 3.39
 *   草食 / 肉食 は 1.18 と明度差が小さいので、色相（青緑と橙）と大きさで区別する
 */
export const COLORS = {
  background: '#06090b',
  plant: '#2a6b42',
  herbivore: '#9fd8c4',
  carnivore: '#ffa878',
  /** 選択中の個体の目印 */
  marker: '#dde6e4',
};

/** 個体の描画上の半径（世界座標の px） */
export const BODY_RADIUS = { herbivore: 3, carnivore: 4.5 };

/**
 * 個体を選択できる最小の拡大率。全体表示では点が 1〜2px しかなく、狙って選べないため。
 * 1 のとき、草食は画面上で直径 6px になる
 */
export const MIN_SELECT_SCALE = 0.8;

/** クリック位置からこの距離（画面上の px）以内の個体を選ぶ */
export const SELECT_RADIUS_PX = 14;

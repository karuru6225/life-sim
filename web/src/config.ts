// 調整用の設定値。Rust を触らずに変えられる値はここに集める。

/**
 * シミュレーションのパラメータ。名前は sim/src/params.rs と一致させること
 * （Rust が知らない名前はブラウザのコンソールに警告が出る）。
 * 各値の意味は params.rs のコメントを参照。
 */
export const SIM_PARAMS = {
  // 世界（リセット時のみ反映）
  world_width: 7200,
  world_height: 4800,
  plant_cell: 32,
  herb_initial: 25000,
  carn_initial: 1600,
  initial_age_spread: 0,
  founder_groups: 0,
  founder_litter_min: 1,
  founder_litter_max: 3,
  founder_invest_min: 0.3,
  founder_invest_max: 0.7,
  plant_initial_ratio: 0.5,

  // 地形（リセット時のみ反映）
  water_fraction: 0.25,
  elevation_scale: 1200,
  fertility_scale: 600,
  fertility_min: 0.3,

  // 気候（リセット時のみ反映）
  temp_mean: 15,
  temp_latitude: 12,
  temp_lapse: 15,
  temp_noise: 3,
  plant_temp_opt: 18,
  plant_temp_width: 15,
  thermo_cost: 0.0004,
  mutation_temp_sigma: 0.5,

  // 季節
  season_length: 2400,
  season_temp: 10,
  rain_dry: 0.3,
  rain_width: 0.08,
  rain_center: 0.5,
  rain_swing: 0.08,
  season_amplitude: 0,

  // 植物
  sunlight: 0.08,
  plant_capacity: 10,
  plant_share_shrub: 0.3,
  plant_share_tree: 0.2,
  woodland_contrast: 0.8,
  shrub_quality: 0.85,
  tree_quality: 0.7,
  reach_shrub: 0.85,
  reach_tree: 1.05,
  reach_width: 0.15,
  graze_floor: 0.1,
  graze_floor_power: 1.5,
  graze_floor_min: 0.03,
  far_view: 320,
  far_view_tree: 1.5,

  // 共通
  move_cost: 0.006,
  move_size_power: 0.7,
  vision_cost: 0.00008,
  speed_upkeep: 0.002,
  max_turn: 0.5,
  turn_accel: 0.33,
  turn_cost: 0.002,
  turn_noise: 0.15,
  cover_strength: 0.85,
  motion_reveal: 0.8,
  conceal_size_power: 0.3333,
  body_energy: 3,
  brain_cost: 0.0001,
  memory_cost: 0.0001,
  memory_initial: 300,
  memory_wiring: 0,
  nest_build_cost: 0.005,
  nest_material: 0.1,
  nest_material_need: 3,
  nest_decay: 0.0005,
  nest_insulation: 0.7,
  nest_hide: 0.9,
  nest_wiring: 0,
  carn_family_wiring: 0,
  feed_wiring: 0,
  energy_cap_ratio: 2,
  birth_spread: 6,
  rest_metabolism: 0.5,
  personal_space: 5,
  separation_weight: 1.5,

  // 弱り
  frail_energy: 0.3,
  frail_age: 0.7,
  frail_speed: 0.5,

  // 繁殖シグナル
  call_cell: 128,
  signal_cost: 0.001,
  signal_exposure: 0.5,

  // 成長
  min_birth_growth: 0.05,
  juvenile_speed: 0.6,
  immobile_growth: 0.2,
  mobile_growth: 0.4,
  feed_rate: 0.02,
  feed_efficiency: 0.7,
  feed_range: 8,
  gestation_load: 0.1,
  fat_load: 0.02,
  fat_insulation: 0.3,

  // 持久力
  herb_cruise: 0.7,
  carn_cruise: 0.7,
  stamina_sprint: 40,
  stamina_recover: 120,

  // 狩り
  hunt_success: 0.9,
  escape_strength: 0.15,
  hunt_easy_ratio: 0.7,
  hunt_impossible_ratio: 1.5,
  guard_radius: 12,
  guard_strength: 0.15,
  counter_start_ratio: 1.2,
  counter_damage: 8,
  carrion_decay: 0.01,
  carn_scavenge_bite: 0.2,
  carrion_spoil: 150,
  carrion_sick: 0.05,
  carrion_sick_damage: 2,
  carrion_fight: 0.02,
  carrion_fight_damage: 1,
  prey_pref_softness: 0.5,
  attack_cost: 0.3,
  attack_cooldown: 10,

  // 警戒（草食）
  vigilance_min_range: 0.5,
  vigilance_bite_cost: 0.5,

  // 肉食どうしの協力と干渉
  coop_radius: 20,
  coop_strength: 0.3,
  coop_max_allies: 4,
  carn_interference: 0.8,

  // 同じ種類の敵との争い
  fight_range: 6,
  fight_cost: 0.3,
  fight_damage: 3,
  fight_backlash: 0.25,
  fight_size_power: 2,
  initial_attack_bias: -1,
  initial_attack_spread: 0.6,
  initial_kin_attack_bias: -1.2,
  initial_kin_attack_spread: 0.6,

  // 系統
  compat_full_distance: 1.2,
  compat_zero_distance: 2.5,
  hue_similar: 0.05,

  // 変異
  mutation_rate: 0.02,
  mutation_trait_rate: 0.1,
  mutation_carn_size_sigma: 0.12,
  mutation_trait_sigma: 0.05,
  mutation_weight_sigma: 0.1,
  mutation_neuron: 0.003,
  mutation_hue_sigma: 0.01,

  // 草食（形質は初期個体の値。以後は遺伝子として進化する）
  herb_initial_size: 1,
  herb_initial_speed: 1.3,
  herb_initial_litter: 1,
  herb_initial_vision: 30,
  herb_mate_range: 12,
  habitat_tolerance: 0.15,
  habitat_limit: 0.3,
  herb_bite: 0.12,
  herb_efficiency: 0.6,
  herb_digest_size_power: 0.25,
  herb_metabolism: 0.005,
  herb_repro_threshold: 10,
  herb_birth_cost: 1,
  herb_max_age: 2400,
  herb_initial_fov: 300,
  herb_repro_rest: 600,
  herb_gestation: 150,
  herb_maturity: 600,
  herb_initial_energy: 5,

  // 肉食（形質は初期個体の値。以後は遺伝子として進化する）
  carn_initial_size: 1,
  carn_initial_speed: 1.8,
  carn_initial_litter: 1,
  carn_initial_vision: 48,
  carn_mate_range: 20,
  carn_attack_range: 6,
  carn_efficiency: 0.3,
  carn_metabolism: 0.0074,
  carn_repro_threshold: 16,
  carn_birth_cost: 2,
  carn_max_age: 3600,
  carn_initial_fov: 120,
  carn_repro_rest: 900,
  carn_gestation: 200,
  carn_maturity: 900,
  carn_initial_energy: 8,
};

export type SimParams = typeof SIM_PARAMS;

export const SEED = 1;

/** 日射量スライダーの範囲 */
export const SUNLIGHT_RANGE = { min: 0, max: 0.2, step: 0.005 };

/** 等倍のときの 1 秒あたりの tick 数 */
export const TICK_HZ = 30;

/**
 * 日射量の自動調整。計算が重くなったら日射量を少しずつ下げ、個体数を減らして計算量を抑える。
 * 軽くなったら、スライダーで設定した値まで少しずつ戻す。
 * 日射量を変えてから個体数が変わるまで数百〜数千 tick の遅れがあるので、急に動かすと行き過ぎて
 * 個体数が大きく揺れる。そのため変化はゆっくりにしている
 */
/**
 * 世界の広さの選択肢。初期の個体数は面積に比例させる（密度が同じになるように）。
 * 中が SIM_PARAMS の既定値。大は計算が重く、再生が遅くなる
 */
export const WORLD_SIZES = {
  small: { label: '小', width: 4800, height: 3200, herbivores: 11000, carnivores: 700 },
  medium: { label: '中', width: 7200, height: 4800, herbivores: 25000, carnivores: 1600 },
  large: { label: '大', width: 10800, height: 7200, herbivores: 56000, carnivores: 3600 },
} as const;
export type WorldSize = keyof typeof WORLD_SIZES;

/** 日射量の自動調整（既定はオフ。HUD のチェックで入れる） */
export const AUTO_SUNLIGHT = {
  /** 起動時に自動調整を使うか */
  enabledByDefault: false,
  /**
   * 目標の計算時間（ms/tick）。等倍の 1 tick に使える時間（33ms）より長めにとり、多少遅くなっても日射は削らない。
   * 目標を短くしすぎると、日射を大きく削って草食・肉食を飢えさせてしまう（2 層の脳にしてから、肉食が絶滅する原因になっていた）
   */
  targetMs: 40,
  /** 計算時間の移動平均の重み（0.5 秒ごとに更新）。小さいほど一時的な揺れに反応しない */
  smoothing: 0.2,
  /**
   * 1 tick あたりに変える割合（30 tick で 0.25%）。
   * 生態系の反応の遅れは tick 単位なので、実時間ではなく tick 数に比例させる
   * （実時間で決めると、倍速のときに生態系から見て調整が速すぎる）
   */
  stepPerTick: 0.0025 / 30,
  /** 1 回（0.5 秒ごと）に変える割合の上限。最大速度で一度に大きく動かないように */
  maxStep: 0.01,
  /** これより下げない（設定値に対する割合）。それ以上重いときは、日射を削らずに再生が遅くなる */
  minFactor: 0.7,
  /** 目標のこの割合を下回ったら戻し始める（上げ下げを繰り返さないための余裕） */
  recoverBelow: 0.8,
};

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
  /** 地図「育ちやすさ」の最も育ちやすい色（最も育ちにくい landPoor に対して 4.5:1） */
  growthHigh: '#6b7f2a',
  /** 死体。植物の上に重ねるので、植物の色に対しても 3:1 を確保（3.07:1、地の色に対して 9.6:1） */
  carrion: '#d9a0e0',
  /** 巣（枝と草を編んだ枯れ草の色）。植物の色に対して 3.71:1、地の色に対して 11.6:1 */
  nest: '#e0c27a',
  herbivore: '#9fd8c4',
  carnivore: '#ffa878',
  /** 選択中の個体の目印 */
  marker: '#dde6e4',
  /**
   * 地形。水域は青、陸は肥沃度に応じて痩せた陸〜肥沃な陸の間（植物の緑はこの上に重なる）。
   * 水域と陸の明るさの差は 1.6〜1.8:1 と小さいが、広い面で、青と黒・緑の色相の違いで見分けられる
   */
  water: '#163a58',
  landPoor: '#06090b',
  landRich: '#0e140f',
  /**
   * 気温の表示（寒い → 中間 → 暑い）。個体の色に対してどれも 5:1 以上。
   * 寒い陸と水域が同じ青に見えないよう、気温の表示では水域を黒に近い色にし、寒い色は青紫に寄せる
   */
  tempCold: '#2c3a7a',
  tempMild: '#2a3029',
  tempHot: '#6e2e18',
  tempWater: '#030405',
  /**
   * 林と草原の表示（草原 → 林）。明るさはほぼ同じ（1.5:1）で、色相（枯れ草の黄土色と葉の緑）で見分ける。
   * 個体の色に対して、草原で草食 5.6:1・肉食 4.8:1、林で草食 8.5:1・肉食 7.2:1
   */
  grassland: '#5a4618',
  woodland: '#0f3520',
  /**
   * 標高の表示。陸は低地（暗い緑）→ 高地（灰茶）、水域は浅い（青）→ 深い（暗い青）。
   * 最も明るい高地でも、個体の色に対して草食 4.8:1・肉食 4.1:1・死体 3.7:1。低地と高地は 2.3:1、浅い水と低地は 2.2:1（色相も違う）
   */
  lowland: '#101e15',
  highland: '#5a5249',
  shallow: '#26557a',
  deep: '#08131f',
};

/** 強調中の種類を描く、画面上の最小の半径（px）。全体表示まで引いても分布が見えるように */
/** 個体を描く画面上の最小の半径（px）。小さな種（大きさ 0.1 など）を引いた表示でも見失わないように */
export const MIN_BODY_RADIUS_PX = 1;
export const HIGHLIGHT_MIN_RADIUS_PX = 2.5;

/** 「薄くする」強調のとき、強調していない種類の不透明度 */
export const DIMMED_ALPHA = 0.15;

/**
 * 個体の描画上の半径（世界座標の px）。大きさの遺伝子 1 のときの値で、実際は大きさに比例する。
 * 肉食は数が少なく埋もれやすいので、見やすさのため 1.5 倍で描く（同じ種類の中での大小関係は正しい）
 */
export const BODY_RADIUS = { herbivore: 3, carnivore: 4.5 };

/**
 * 系統色（標識色の遺伝子）の表示に使う色の明るさ（WCAG の相対輝度）。
 * 色相によって明るさが変わると、植物の上で見えにくい色が出るので、全色相でこの輝度にそろえる。
 * 0.46 のとき、植物満杯（#2a6b42）に対して 3.11:1、地色に対して約 9.7:1
 */
export const HUE_LUMINANCE = 0.46;
export const HUE_STEPS = 72;

/**
 * 個体を選択できる最小の拡大率。全体表示では点が 1〜2px しかなく、狙って選べないため。
 * 1 のとき、草食は画面上で直径 6px になる
 */
export const MIN_SELECT_SCALE = 0.8;

/** クリック位置からこの距離（画面上の px）以内の個体を選ぶ */
export const SELECT_RADIUS_PX = 14;

/**
 * 選択中の個体が死んだとき、代わりに近くの個体を自動で選ぶ。
 * 死んだ場所から radius（世界座標の px）以内で、同じ種類の最寄りを優先する
 */
export const AUTO_RESELECT = { delayMs: 2000, radius: 300 };

//! 調整用パラメータ。
//!
//! 既定値はここに書くが、ブラウザでは web/src/config.ts の値で上書きされる。
//! JS からは名前（文字列）で設定するので、ここで名前を変えたら config.ts も合わせること。
//!
//! 単位: 距離は px、時間は tick、エネルギーは無次元の量。

macro_rules! params {
    ($( $(#[doc = $doc:literal])* $name:ident = $default:expr, )*) => {
        #[derive(Clone, Debug)]
        pub struct Params {
            $( $(#[doc = $doc])* pub $name: f32, )*
        }

        impl Default for Params {
            fn default() -> Self {
                Params { $( $name: $default, )* }
            }
        }

        impl Params {
            /// 名前で値を設定する。知らない名前なら false
            pub fn set(&mut self, name: &str, value: f32) -> bool {
                match name {
                    $( stringify!($name) => { self.$name = value; true } )*
                    _ => false,
                }
            }
        }
    };
}

params! {
    // ---- 世界（リセット時のみ反映） ----
    /// 世界の幅
    world_width = 7200.0,
    /// 世界の高さ
    world_height = 4800.0,
    /// 植物のマス目の一辺
    plant_cell = 32.0,
    /// 初期の草食の数
    herb_initial = 25000.0,
    /// 初期の肉食の数
    carn_initial = 1500.0,
    /// 初期の植物量（plant_capacity に対する割合）
    plant_initial_ratio = 0.5,

    // ---- 植物 ----
    /// 1 マスに 1 tick で届く日射エネルギー。生態系全体のエネルギー流入量を決める
    sunlight = 0.08,
    /// 1 マスに溜まる植物量の上限。満杯に近いほど日射を受け取れなくなる
    plant_capacity = 10.0,

    // ---- 共通 ----
    /// 移動のエネルギー消費（大きさ × 速度の 2 乗に掛ける）
    move_cost = 0.006,
    /// 視野 1px あたりの代謝（目と脳の維持費）
    vision_cost = 0.00008,
    /// 1 tick に曲がれる最大の角度（ラジアン）。大きい個体ほど小回りが利かない。
    /// 個体が生まれるときに読むので、変更は以後に生まれた個体から効く
    max_turn = 0.5,
    /// 進む向きに毎 tick 加わる揺れ（ラジアン）
    turn_noise = 0.15,
    /// 草による隠れの強さ。草が満杯のマスにいる相手は、視野が (1 - この値) 倍に縮んで見える
    cover_strength = 0.85,
    /// 蓄えられるエネルギーの上限（繁殖に必要な量に対する倍率）
    energy_cap_ratio = 2.0,
    /// 生まれた子の出現位置のばらつき
    birth_spread = 6.0,

    // ---- 変異 ----
    /// 遺伝子 1 個あたりの変異確率
    mutation_rate = 0.02,
    /// 形質の変異の大きさ（現在値に対する割合の標準偏差）
    mutation_trait_sigma = 0.05,
    /// 脳の重みの変異の大きさ（標準偏差）
    mutation_weight_sigma = 0.1,
    /// 標識色の変異の大きさ（毎回必ず漂う。色相 1 周 = 1）
    mutation_hue_sigma = 0.01,

    // ---- 草食（形質は初期個体の値。以後は遺伝子として進化する） ----
    /// 初期の大きさ
    herb_initial_size = 1.0,
    /// 初期の最高速度
    herb_initial_speed = 1.3,
    /// 初期の視野
    herb_initial_vision = 30.0,
    /// 繁殖相手を探せる距離。この距離以内に見えている草食がいないと繁殖できない。0 以下なら無性生殖
    herb_mate_range = 12.0,
    /// 1 tick に食べる植物量の上限（大きさ 1 のとき。大きさに比例）
    herb_bite = 0.12,
    /// 食べた植物のうち自分のエネルギーになる割合（残りは熱として失われる）
    herb_efficiency = 0.6,
    /// 生きているだけで 1 tick に失うエネルギー（大きさ 1 のとき。大きさの 0.75 乗に比例。視野の分は別）
    herb_metabolism = 0.0076,
    /// 繁殖に必要なエネルギー（大きさ 1 のとき。大きさに比例）
    herb_repro_threshold = 10.0,
    /// 繁殖 1 回で失うエネルギー（大きさ 1 のとき。大きさに比例）
    herb_birth_cost = 1.0,
    /// 寿命（大きさ 1 のとき。大きさの平方根に比例）
    herb_max_age = 2400.0,
    /// 初期個体のエネルギー
    herb_initial_energy = 5.0,

    // ---- 肉食（形質は初期個体の値。以後は遺伝子として進化する） ----
    /// 初期の大きさ
    carn_initial_size = 1.0,
    /// 初期の最高速度
    carn_initial_speed = 1.8,
    /// 初期の視野
    carn_initial_vision = 48.0,
    /// 繁殖相手を探せる距離。肉食は数が少なく相手を見つけにくいので、草食より広くする。0 以下なら無性生殖
    carn_mate_range = 20.0,
    /// 獲物を捕まえられる距離（大きさ 1 のとき。大きさに比例）
    carn_attack_range = 6.0,
    /// 襲える獲物の大きさの上限（自分の大きさに対する倍率）
    prey_size_ratio = 1.3,
    /// 食べた獲物のエネルギーのうち自分のものになる割合
    carn_efficiency = 0.5,
    /// 生きているだけで 1 tick に失うエネルギー（大きさ 1 のとき。視野の分は別）
    carn_metabolism = 0.011,
    /// 繁殖に必要なエネルギー（大きさ 1 のとき）
    carn_repro_threshold = 16.0,
    /// 繁殖 1 回で失うエネルギー（大きさ 1 のとき）
    carn_birth_cost = 2.0,
    /// 寿命（大きさ 1 のとき）
    carn_max_age = 3600.0,
    /// 初期個体のエネルギー
    carn_initial_energy = 8.0,
}

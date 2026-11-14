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
    world_width = 4800.0,
    /// 世界の高さ
    world_height = 3200.0,
    /// 植物のマス目の一辺
    plant_cell = 32.0,
    /// 初期の草食の数
    herb_initial = 20000.0,
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
    /// 移動のエネルギー消費（速度の 2 乗に掛ける）
    move_cost = 0.004,
    /// 生まれた子の出現位置のばらつき
    birth_spread = 6.0,

    // ---- 草食 ----
    /// 天敵が見える距離
    herb_vision = 30.0,
    /// 草を食べながらの移動速度
    herb_graze_speed = 0.4,
    /// 草を探して移動するときの速度
    herb_speed = 0.9,
    /// 逃げるときの速度
    herb_flee_speed = 1.3,
    /// 1 tick に食べる植物量の上限
    herb_bite = 0.12,
    /// 食べた植物のうち自分のエネルギーになる割合（残りは熱として失われる）
    herb_efficiency = 0.6,
    /// 生きているだけで 1 tick に失うエネルギー
    herb_metabolism = 0.01,
    /// このエネルギーに達したら繁殖する
    herb_repro_threshold = 10.0,
    /// 繁殖 1 回で失うエネルギー
    herb_birth_cost = 1.0,
    /// 寿命
    herb_max_age = 2400.0,
    /// 初期個体のエネルギー
    herb_initial_energy = 5.0,

    // ---- 肉食 ----
    /// 獲物が見える距離
    carn_vision = 48.0,
    /// 獲物を捕まえられる距離
    carn_attack_range = 6.0,
    /// 獲物がいないときの移動速度
    carn_wander_speed = 0.6,
    /// 獲物を追うときの速度
    carn_chase_speed = 1.8,
    /// 食べた獲物のエネルギーのうち自分のものになる割合
    carn_efficiency = 0.5,
    /// 生きているだけで 1 tick に失うエネルギー
    carn_metabolism = 0.012,
    /// このエネルギーに達したら繁殖する
    carn_repro_threshold = 16.0,
    /// 繁殖 1 回で失うエネルギー
    carn_birth_cost = 2.0,
    /// 寿命
    carn_max_age = 3600.0,
    /// 初期個体のエネルギー
    carn_initial_energy = 8.0,
}

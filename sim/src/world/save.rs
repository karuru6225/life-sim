//! 世界の保存と読み込み。
//!
//! 世界の状態をすべてバイト列にする: パラメータ、tick、乱数の状態、帳簿と記録、植物と死体、全個体。
//! 乱数の状態も含むので、読み込んで再開した歴史は、保存しなかった場合とまったく同じに続く。
//! 地形は乱数の種とパラメータから作り直し、近傍探索のグリッドや呼び声の場は次の tick で作り直す。
//!
//! 互換性: 先頭に形式の番号と、遺伝子・脳の構造（形質の数、入力・出力の数など）を書く。
//! これが今のコードと違えば、遺伝子を正しく読めないので読み込みを断る。
//! パラメータは名前と値の組で書くので、パラメータが増えただけなら古いデータも読める（増えた分は既定値）。
//!
//! 長期の統計の記録（history.rs）は末尾に、バイト数を先に書いて置く。記録の並びが今のコードと違えば、読み飛ばして記録を空から始める
//! （記録は世界の進み方に関わらないので、なくても歴史は同じに続く）。形式 2（記録のない形式）も読める。
//!
//! 遺伝子は 1 体あたり千数百個あり、使っていないニューロンの重みは 0 なので、
//! 0 でない値だけを書き、どこが 0 でないかはビットの並びで示す。

use super::*;
use crate::genome::{H1, H2, N_OUT, N_TRAITS};

/// ファイルの先頭の印
const MAGIC: &[u8; 8] = b"LSIMWRLD";
/// 形式の番号。保存する中身の並びを変えたら上げる
pub const FORMAT_VERSION: u32 = 8;
/// 読み込める最も古い形式の番号（形式 6 までは脳の入力に体力がなく、遺伝子の並びが違うので読めない）
/// 形式 7 は、狩りで獲物が天敵に気づいていたかの集計がない。読むときは 0 にする
const OLDEST_FORMAT: u32 = 7;
/// 死因の数（Census の配列の長さ）
const N_CAUSES: usize = 8;

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.f32(x);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.at + n > self.data.len() {
            return Err("データが途中で切れている".into());
        }
        let s = &self.data[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32s(&mut self, expected: usize, what: &str) -> Result<Vec<f32>, String> {
        let n = self.u32()? as usize;
        if n != expected {
            return Err(format!("{what} の数が合わない（保存 {n}、今の設定 {expected}）"));
        }
        (0..n).map(|_| self.f32()).collect()
    }
}

/// 遺伝子と脳の構造。保存時と読み込み時で一致しないと遺伝子を読めない
fn layout() -> [u32; 7] {
    [N_TRAITS as u32, N_IN as u32, N_OUT as u32, H1 as u32, H2 as u32, GENOME_LEN as u32, N_CAUSES as u32]
}

impl World {
    /// 世界の状態をすべてバイト列にする（tick と tick の間に呼ぶこと）
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer(Vec::with_capacity(self.count() * 2_500 + self.plants.biomass.len() * 12 + 4096));
        w.0.extend_from_slice(MAGIC);
        w.u32(FORMAT_VERSION);
        for v in layout() {
            w.u32(v);
        }
        w.u64(self.seed);
        let entries = self.params.entries();
        w.u32(entries.len() as u32);
        for (name, value) in entries {
            w.u8(name.len() as u8);
            w.0.extend_from_slice(name.as_bytes());
            w.f32(value);
        }

        w.u64(self.tick);
        w.u32(self.next_id);
        w.u64(self.matings);
        w.u64(self.failed_matings);
        let (state, inc) = self.rng.state();
        w.u64(state);
        w.u64(inc);

        let l = &self.ledger;
        for v in [
            l.initial,
            l.sunlight_in,
            l.lost_metabolism,
            l.lost_digestion,
            l.lost_birth,
            l.lost_death,
            l.herb_gain,
            l.carn_gain,
            l.lost_decay,
            l.lost_build,
        ] {
            w.f64(v);
        }
        let c = &self.census;
        for k in 0..2 {
            for cause in 0..N_CAUSES {
                w.u64(c.deaths[k][cause]);
                w.u64(c.juvenile_deaths[k][cause]);
            }
            w.u64(c.births[k]);
            w.u64(c.fights[k]);
            w.u64(c.kin_fights[k]);
            w.u64(c.builds[k]);
        }
        w.u64(c.hunts);
        w.u64(c.kills);
        w.u64(c.hunt_noticed);
        w.u64(c.kill_noticed);
        for v in [c.hunt_ratio, c.hunt_guards, c.hunt_allies, c.hunt_rivals, c.hunt_chance, c.kill_eaters, c.hunt_escape] {
            w.f64(v);
        }
        w.u32(self.recent_deaths.len() as u32);
        for d in &self.recent_deaths {
            w.u64(d.tick);
            w.u8(d.kind);
            w.u8(d.cause);
            w.f32(d.hue);
        }

        // 合計は層の和を足し込んで保っているので、和を取り直すと丸めが変わる。そのまま書く
        w.f32s(&self.plants.biomass);
        for layer in &self.plants.layer {
            w.f32s(layer);
        }
        w.f32s(&self.carrion);
        w.f32s(&self.carrion_age);
        w.f32s(&self.nest);
        for &o in &self.nest_owner {
            w.u32(o);
        }

        let n = self.count();
        w.u32(n as u32);
        for i in 0..n {
            w.u32(self.id[i]);
            w.f32(self.xs[i]);
            w.f32(self.ys[i]);
            w.f32(self.hx[i]);
            w.f32(self.hy[i]);
            w.f32(self.energy[i]);
            w.u32(self.age[i]);
            w.u8(self.kind[i]);
            w.u8(self.state[i]);
            w.u32(self.generation[i]);
            w.u32(self.children[i]);
            w.f32(self.growth[i]);
            // いまの大きさは成長に合わせて 8 tick ごとに計算し直しているので、成長度とずれていることがある。そのまま保存する
            w.f32(self.traits[i].size);
            w.u16(self.cooldown[i]);
            w.u16(self.rest[i]);
            w.u8(self.attacked[i]);
            w.u16(self.alone[i]);
            w.f32(self.vigilance[i]);
            w.f32(self.natal[i]);
            w.f32(self.cover[i]);
            w.f32(self.signal[i]);
            w.u32(self.parents[i][0]);
            w.u32(self.parents[i][1]);
            let life = &self.life[i];
            w.f32(life.eaten);
            w.u32(life.kills);
            w.u32(life.hunts);
            w.f32(life.distance);
            w.u32(life.matings);
            w.u32(life.failed);
            let m = &self.memory[i];
            w.f32(m.x);
            w.f32(m.y);
            w.f32(m.value);
            w.u64(m.tick);
            w.u32(self.home[i]);
            w.f32(self.vel[i].0);
            w.f32(self.vel[i].1);
            w.f32(self.stamina[i]);
            // 遺伝子: 0 でない値の位置のビット列と、その値
            let g = self.genome(i);
            let mut bits = vec![0u8; GENOME_LEN.div_ceil(8)];
            for (k, v) in g.iter().enumerate() {
                if v.to_bits() != 0 {
                    bits[k / 8] |= 1 << (k % 8);
                }
            }
            w.0.extend_from_slice(&bits);
            for v in g {
                if v.to_bits() != 0 {
                    w.f32(*v);
                }
            }
        }
        // 長期の統計の記録。読み飛ばせるよう、先にバイト数を書く
        let mut h = Writer(Vec::new());
        write_history(&mut h, &self.history);
        w.u32(h.0.len() as u32);
        w.0.extend_from_slice(&h.0);
        w.0
    }

    /// save() のバイト列から世界を作り直す。戻り値の 2 つ目は、今のコードが知らないパラメータの名前（無視したもの）
    pub fn load(data: &[u8]) -> Result<(World, Vec<String>), String> {
        let mut r = Reader { data, at: 0 };
        if r.take(8)? != MAGIC {
            return Err("life-sim の世界のデータではない".into());
        }
        let version = r.u32()?;
        if !(OLDEST_FORMAT..=FORMAT_VERSION).contains(&version) {
            return Err(format!("保存形式の番号が違う（保存 {version}、今のコード {FORMAT_VERSION}）"));
        }
        let saved_layout: Vec<u32> = (0..layout().len()).map(|_| r.u32()).collect::<Result<_, _>>()?;
        if saved_layout != layout() {
            return Err(format!(
                "遺伝子と脳の構造が違う（保存 {saved_layout:?}、今のコード {:?}。並びは 形質・入力・出力・中間1・中間2・遺伝子長・死因の数）",
                layout()
            ));
        }
        let seed = r.u64()?;
        let mut params = Params::default();
        let mut unknown = Vec::new();
        for _ in 0..r.u32()? {
            let len = r.u8()? as usize;
            let name = String::from_utf8(r.take(len)?.to_vec()).map_err(|_| "パラメータの名前が壊れている")?;
            let value = r.f32()?;
            if !params.set(&name, value) {
                unknown.push(name);
            }
        }

        let mut world = World::empty(seed, params);
        world.tick = r.u64()?;
        world.next_id = r.u32()?;
        world.matings = r.u64()?;
        world.failed_matings = r.u64()?;
        let (state, inc) = (r.u64()?, r.u64()?);
        world.rng = Rng::from_state(state, inc);

        let l = &mut world.ledger;
        l.initial = r.f64()?;
        l.sunlight_in = r.f64()?;
        l.lost_metabolism = r.f64()?;
        l.lost_digestion = r.f64()?;
        l.lost_birth = r.f64()?;
        l.lost_death = r.f64()?;
        l.herb_gain = r.f64()?;
        l.carn_gain = r.f64()?;
        l.lost_decay = r.f64()?;
        l.lost_build = r.f64()?;
        let c = &mut world.census;
        for k in 0..2 {
            for cause in 0..N_CAUSES {
                c.deaths[k][cause] = r.u64()?;
                c.juvenile_deaths[k][cause] = r.u64()?;
            }
            c.births[k] = r.u64()?;
            c.fights[k] = r.u64()?;
            c.kin_fights[k] = r.u64()?;
            c.builds[k] = r.u64()?;
        }
        c.hunts = r.u64()?;
        c.kills = r.u64()?;
        if version >= 8 {
            c.hunt_noticed = r.u64()?;
            c.kill_noticed = r.u64()?;
        }
        c.hunt_ratio = r.f64()?;
        c.hunt_guards = r.f64()?;
        c.hunt_allies = r.f64()?;
        c.hunt_rivals = r.f64()?;
        c.hunt_chance = r.f64()?;
        c.kill_eaters = r.f64()?;
        c.hunt_escape = r.f64()?;
        for _ in 0..r.u32()? {
            let d = DeathRecord { tick: r.u64()?, kind: r.u8()?, cause: r.u8()?, hue: r.f32()? };
            world.recent_deaths.push_back(d);
        }

        let cells = world.plants.biomass.len();
        world.plants.biomass = r.f32s(cells, "植物のマス")?;
        for k in 0..crate::plants::LAYERS {
            world.plants.layer[k] = r.f32s(cells, "植物の層のマス")?;
        }
        world.carrion = r.f32s(cells, "死体のマス")?;
        world.carrion_age = r.f32s(cells, "死体の古さのマス")?;
        world.nest = r.f32s(cells, "巣のマス")?;
        for o in world.nest_owner.iter_mut() {
            *o = r.u32()?;
        }

        let n = r.u32()? as usize;
        let max_turn = world.params.max_turn;
        let mut genome = vec![0.0f32; GENOME_LEN];
        for _ in 0..n {
            let id = r.u32()?;
            let (x, y, hx, hy, energy) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?);
            let age = r.u32()?;
            let (kind, st) = (r.u8()?, r.u8()?);
            let (generation, children) = (r.u32()?, r.u32()?);
            let (growth, size) = (r.f32()?, r.f32()?);
            let (cooldown, rest, attacked, alone) = (r.u16()?, r.u16()?, r.u8()?, r.u16()?);
            let (vigilance, natal, cover, signal) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
            let parents = [r.u32()?, r.u32()?];
            let life = Life {
                eaten: r.f32()?,
                kills: r.u32()?,
                hunts: r.u32()?,
                distance: r.f32()?,
                matings: r.u32()?,
                failed: r.u32()?,
            };
            let memory = Memory { x: r.f32()?, y: r.f32()?, value: r.f32()?, tick: r.u64()? };
            let home = r.u32()?;
            let vel = (r.f32()?, r.f32()?);
            let stamina = r.f32()?;
            if home != NO_HOME && home as usize >= cells {
                return Err(format!("覚えている巣のマスが壊れている（{home}）"));
            }
            let bits = r.take(GENOME_LEN.div_ceil(8))?.to_vec();
            for (k, v) in genome.iter_mut().enumerate() {
                *v = if bits[k / 8] & (1 << (k % 8)) != 0 { r.f32()? } else { 0.0 };
            }
            if kind > CARNIVORE {
                return Err(format!("個体の種類が壊れている（{kind}）"));
            }

            let mut traits = genome::traits(&genome, max_turn);
            traits.set_size(size, max_turn);
            world.id.push(id);
            world.xs.push(x);
            world.ys.push(y);
            world.hx.push(hx);
            world.hy.push(hy);
            world.energy.push(energy);
            world.age.push(age);
            world.kind.push(kind);
            world.state.push(st);
            world.generation.push(generation);
            world.children.push(children);
            world.genomes.extend_from_slice(&genome);
            world.traits.push(traits);
            world.growth.push(growth);
            world.cooldown.push(cooldown);
            world.rest.push(rest);
            world.attacked.push(attacked);
            world.alone.push(alone);
            world.memory.push(memory);
            world.home.push(home);
            world.vel.push(vel);
            world.stamina.push(stamina);
            world.vigilance.push(vigilance);
            world.natal.push(natal);
            world.cover.push(cover);
            world.signal.push(signal);
            world.parents.push(parents);
            world.life.push(life);
            world.alive.push(true);
        }
        let len = r.u32()? as usize;
        let bytes = r.take(len)?;
        match read_history(&mut Reader { data: bytes, at: 0 }) {
            Ok(Some(h)) => world.history = h,
            // 記録の並びが違う、または壊れている: 記録は空から始める
            _ => world.rebase_history(),
        }
        if r.at != data.len() {
            return Err(format!("データの末尾に余分なバイトがある（{} バイト）", data.len() - r.at));
        }

        // 季節の気温と植物の育ちやすさは CLIMATE_EVERY tick ごとに更新している。最後に更新したときの値に戻す
        let now = world.tick;
        world.tick = if now == 0 { 0 } else { (now - 1) / CLIMATE_EVERY * CLIMATE_EVERY };
        world.update_climate();
        world.tick = now;
        Ok((world, unknown))
    }
}

fn write_history(w: &mut Writer, h: &History) {
    use super::history::KIND_LEN;
    w.u32(KIND_LEN as u32);
    w.u32(h.stride);
    w.u32(h.pending_count);
    w.u64(h.last_tick);
    for k in 0..2 {
        w.u64(h.last_births[k]);
        for &d in &h.last_deaths[k] {
            w.u64(d);
        }
    }
    let record = |w: &mut Writer, r: &super::history::Record| {
        w.u64(r.tick);
        w.u64(r.span);
        for v in r.values.iter().flatten() {
            w.f32(*v);
        }
    };
    w.u32(h.records.len() as u32);
    for r in &h.records {
        record(w, r);
    }
    w.u8(h.pending.is_some() as u8);
    if let Some(p) = &h.pending {
        record(w, p);
    }
}

/// 長期の統計の記録を読む。記録の並び（種類ごとの値の数）が今のコードと違えば None
fn read_history(r: &mut Reader) -> Result<Option<History>, String> {
    use super::history::{KIND_LEN, Record};
    if r.u32()? as usize != KIND_LEN {
        return Ok(None);
    }
    let mut h = History::new();
    h.stride = r.u32()?;
    h.pending_count = r.u32()?;
    h.last_tick = r.u64()?;
    for k in 0..2 {
        h.last_births[k] = r.u64()?;
        for d in h.last_deaths[k].iter_mut() {
            *d = r.u64()?;
        }
    }
    let record = |r: &mut Reader| -> Result<Record, String> {
        let (tick, span) = (r.u64()?, r.u64()?);
        let mut values = [[0.0f32; KIND_LEN]; 2];
        for v in values.iter_mut().flatten() {
            *v = r.f32()?;
        }
        Ok(Record { tick, span, values })
    };
    for _ in 0..r.u32()? {
        h.records.push(record(r)?);
    }
    if r.u8()? != 0 {
        h.pending = Some(record(r)?);
    }
    if r.at != r.data.len() {
        return Ok(None);
    }
    Ok(Some(h))
}

/// ブラウザの保存ファイルと同じ入れ物（圧縮なし）にする: "LIFESIM\n"、見出し（JSON）の長さ（u32）、見出し、世界のバイト列。
/// ブラウザの「読み込み」は、gzip で圧縮していないこの形も読める。commit は保存したコード、saved_at は保存日時（ISO 8601）
pub fn save_file(world: &World, commit: &str, saved_at: &str) -> Vec<u8> {
    let payload = world.save();
    let header = format!(
        "{{\"format\":{FORMAT_VERSION},\"commit\":\"{}\",\"savedAt\":\"{}\",\"tick\":{},\"herbivores\":{},\"carnivores\":{}}}",
        commit.replace(['"', '\\'], ""),
        saved_at.replace(['"', '\\'], ""),
        world.tick,
        world.count_kind(HERBIVORE),
        world.count_kind(CARNIVORE),
    );
    let mut out = Vec::with_capacity(payload.len() + header.len() + 12);
    out.extend_from_slice(b"LIFESIM\n");
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&payload);
    out
}

/// save_file() の入れ物（圧縮なし）から世界を読み込む。戻り値の 2 つ目は、今のコードが知らないパラメータの名前
pub fn load_file(data: &[u8]) -> Result<(World, Vec<String>), String> {
    if !data.starts_with(b"LIFESIM
") || data.len() < 12 {
        return Err("life-sim の保存ファイル（圧縮なし）ではない".into());
    }
    let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let payload = data.get(12 + len..).ok_or("保存ファイルの見出しが壊れている")?;
    World::load(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        let mut p = Params::default();
        p.world_width = 960.0;
        p.world_height = 640.0;
        p.herb_initial = 800.0;
        p.carn_initial = 60.0;
        // 巣も保存と読み込みで続くことを確かめる
        p.nest_wiring = 2.0;
        p
    }

    #[test]
    fn 保存して読み込んだ世界は保存しなかった場合と同じ歴史をたどる() {
        let mut a = World::new(11, params());
        for _ in 0..157 {
            a.step();
        }
        let data = a.save();
        let (mut b, unknown) = World::load(&data).expect("読み込める");
        assert!(unknown.is_empty());
        for _ in 0..200 {
            a.step();
            b.step();
        }
        assert_eq!(a.tick, b.tick);
        assert_eq!(a.id, b.id);
        assert_eq!(a.xs, b.xs);
        assert_eq!(a.energy, b.energy);
        assert_eq!(a.genomes, b.genomes);
        assert_eq!(a.plants.biomass, b.plants.biomass);
        assert_eq!(a.carrion, b.carrion);
        assert_eq!(a.nest, b.nest);
        assert_eq!(a.nest_owner, b.nest_owner);
        assert_eq!(a.home, b.home);
        assert_eq!(a.vel, b.vel);
        assert_eq!(a.stamina, b.stamina);
        assert_eq!(a.ledger.expected_total(), b.ledger.expected_total());
        assert!(a.history.len() > 0, "記録が溜まっている");
        assert_eq!(a.history, b.history, "長期の統計の記録も続く");
    }

    #[test]
    fn 違う形式のデータは読み込まない() {
        let w = World::new(1, params());
        let mut data = w.save();
        data[8] = 99; // 形式の番号
        assert!(World::load(&data).is_err());
        assert!(World::load(b"not a world").is_err());
        let data = w.save();
        assert!(World::load(&data[..data.len() - 3]).is_err(), "途中で切れたデータ");
    }
}

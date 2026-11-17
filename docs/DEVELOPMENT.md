# life-sim 開発ガイド

コードを触る人向けの文書。生態系の決まりは [ECOSYSTEM.md](ECOSYSTEM.md)、設計の経緯と検証の記録は [PLAN.md](PLAN.md)、
今の状況と次の作業は [HANDOVER.md](HANDOVER.md) を見る。

## 1. 構成

```text
sim/                       Rust クレート（WASM とネイティブの両方にビルドする）
  src/lib.rs               JS との境界。extern "C" の関数と、JS に渡す並び（INSPECT / SUMMARY / 系統の統計 / 長期の統計）
  src/world.rs             世界と個体（SoA）。1 tick の処理、狩り・争い・繁殖・死、集計
  src/world/save.rs        世界の保存と読み込み（バイナリ）、保存ファイルの入れ物
  src/world/history.rs     長期の統計の記録（200 tick ごと。600 を超えたら隣どうしをまとめる。保存ファイルにも入る）
  src/genome.rs            遺伝子の並び、脳（入力・中間 2 層・出力）、初期配線、交叉と変異
  src/params.rs            調整用パラメータ（名前で設定できる。既定値はここ）
  src/grid.rs              近傍探索の一様グリッド
  src/plants.rs            植物のグリッド
  src/terrain.rs           地形（水・肥沃さ・気温）
  src/rng.rs               シード付き乱数（決定論の要）
  examples/headless.rs     画面なしで回し、個体数・形質・群れの度合いの推移を出す（調整の主力）
  examples/diagnose.rs     肉食の生死と狩りの内訳を区間ごとに出す
  examples/herb_size.rs    草食を大人の大きさで段階に分け、段階ごとの誕生・襲われた数・死因を出す
  examples/seasonal.rs     季節の効き方を見る
  examples/resume.rs       保存ファイルから画面なしで続きを進める
web/src/
  main.ts                  エントリ。描画ループ、選択、世界の広さ、保存と読み込み
  worker.ts                WASM を読み込んで sim を回す Web Worker。長期の統計の記録に新しい標本が入るたびに差分を送る
  protocol.ts              メインと Worker の間のメッセージ型、INSPECT / LINEAGE / DEATH の並び
  config.ts                パラメータ（SIM_PARAMS）、色、世界の広さの選択肢、表示の設定
  render/                  PixiJS の描画（個体・植物・地形・カメラ・選択の印）
  ui/                      左のパネル（hud）、個体の詳細（inspector）、系統の統計（lineage）、統計のウインドウ（stats）、脳のウインドウ（brain）、浮かぶウインドウの共通の動き（floating）
scripts/                   WASM のコピー、コミットのハッシュの取得（ビルド時）
.cargo/config.toml         WASM のビルドで SIMD（simd128）を有効にする
.github/workflows/pages.yml  main への push で GitHub Pages に公開
```

## 2. データの流れ

```text
config.ts の SIM_PARAMS ──(名前と値)──▶ worker ──set_param──▶ Params（Rust）
                                         │ init(seed) / step(n)
                                         ▼
                              World（SoA）── fill_render ──▶ 描画用バッファ（位置・色）
                                         │ inspect / summary / lineage_stats（f64 の並び）
                                         ▼
                              worker ──postMessage（transferable）──▶ main ──▶ PixiJS / パネル
```

- 境界を越えるのは数値と WASM メモリ上のアドレスだけ。wasm-bindgen は使っていない。
  JS は `*_ptr()` と `*_len()` で WASM メモリを TypedArray として読む
- 個体の詳細・全体の集計・系統の統計は、Rust 側で固定長の f64 の並びに書き、JS 側は `protocol.ts` の定数で位置を引く

## 3. 1 tick の処理（`World::step`）

1. 近傍探索のグリッドと、鳴き声（呼びかけ）の粗いグリッドを作り直す
2. **判断（plan）**: 全個体が tick の始めの状態だけを見て、知覚 → 脳 → 行動の計画を出す。読み取りだけなので並列にできる
   （`--features parallel` のとき rayon）。結果は順番に依らない
3. **行動（act）**: 計画を、決まった順番（セルの順）で 1 体ずつ実行する。移動・食事・狩り・争い・交配・代謝・死。
   ほかの個体に作用するときは、相手がまだ生きているかを確かめる
4. 生まれた子を加え、死んだ個体を取り除く
5. 30 tick ごとに、季節に応じて気温と植物の育ちやすさを計算し直す

**守ること**: 判断の段で世界を書き換えない。判断の段では世界の乱数を引かない（今は乱数を使っていない。
要るようになったら個体の id と tick から作る。共有の乱数を引くと、並列で結果が変わる）。
「並列でも順番でも同じ結果」は `world.rs` のテスト（判断を並列に計算しても順番に計算しても同じ歴史になる）で確かめている。
このテストは `cargo test --features parallel --manifest-path sim/Cargo.toml` で回さないと、順番どうしの比較になる。

## 4. よくある変更の手順

### パラメータを足す・既定値を変える

1. `sim/src/params.rs` の `params!` に名前・既定値・説明を書く
2. **`web/src/config.ts` の `SIM_PARAMS` にも同じ名前で足す**。ブラウザはこちらの値で Rust の既定値を上書きするので、
   既定値を変えるときは両方を変える（片方だけだと、画面なしの実行とブラウザで結果が変わる）
3. 仕様が変わるなら `docs/ECOSYSTEM.md` を直す

画面なしの実行器では `名前=値` で上書きできる（例 `carn_efficiency=0.35`）。調整の試行はこれで済ませ、決まってから既定値を変える。

### 脳の入力を足す

1. `genome.rs` に `IN_XXX` を足し、`N_IN` を増やす
2. `world.rs` の `plan()` で `input[genome::IN_XXX] = …` を書く（知覚が要るなら `sense()` と `Senses` に足す）。値はおおむね −1〜1 にそろえる
3. 初期配線を決める。行動を最初から変えたくないなら、つながず小さなランダムな重みだけにする（進化に任せる）。
   直接の衝動（「巣へ帰りたい」など）を配線で入れるのは避ける方針（[HANDOVER.md](HANDOVER.md) の「方針」）
4. `web/src/ui/inspector.ts` の `INPUT_LABELS` に名前を足す（並びは `IN_*` と同じ）
5. 脳の構造が変わるので、古い保存ファイルは読めなくなる（保存形式の見出しで弾かれる。下の「保存形式」）

出力を足すときも同じ（`OUT_*`・`N_OUT`、`OUTPUT_LABELS`）。欲求（drive）を足すなら `N_DRIVES` と `mix_drives` も。

### 遺伝子の形質を足す

`G_XXX`・`N_TRAITS`・範囲の定数（`XXX_RANGE`）を足し、`traits()` で読み、`initial()` で初期値を入れる。
変異の幅は `mutate()`。遺伝的距離（`genetic_distance`）に入れるかも決める。
**形質には必ず損を付ける**（入れないと上限に張り付くだけで、進化を観察できない）。

### 個体の詳細・全体の集計・系統の統計に項目を足す

| 並び | Rust | JS |
|---|---|---|
| 個体の詳細 | `lib.rs` の `inspect()` と `INSPECT_LEN` | `protocol.ts` の `INSPECT`、`ui/inspector.ts` |
| 全体の集計 | `lib.rs` の `summary()` と `SUMMARY_LEN` | `worker.ts`、`ui/hud.ts` |
| 系統の統計 | `world.rs` の `lineage_stats()` と `LINEAGE_STATS` | `protocol.ts` の `LINEAGE` と `LINEAGE_LEN`、`ui/lineage.ts` |
| 長期の統計 | `world/history.rs` の `field` と `KIND_LEN`、`lib.rs` の `history_export()`（大きさの分布の段階は `world.rs` の `SIZE_HIST_BINS`） | `protocol.ts` の `HISTORY_FIELD`、`worker.ts` の `syncHistory()`、`ui/stats.ts` |

長さの定数は Rust と JS の両方にあるので、そろえて変える。WASM を作り直さずに画面だけ更新すると、
読む位置がずれて NaN が出る（`npm run build:sim` を忘れない）。

### 死因を足す

`world.rs` の `cause` に足し、`Census` の `deaths` / `juvenile_deaths` の配列（今は `[u64; 8]`）を広げる。
`save.rs` の `N_CAUSES` も別にあるのでそろえる。`protocol.ts` の `DEATH` と、
系統の統計の死因の位置（`LINEAGE` の死因の先頭と `LINEAGE_LEN`、`world.rs` の `LINEAGE_STATS`）も変わる。

## 5. 守るべき不変条件

- **エネルギーを湧かせない**: エネルギーは日射 → 植物 → 草食 → 肉食と一方通行で流れ、残りは代謝・腐敗で消える。
  量を動かすときは必ず帳簿（`ledger`）に記録する。テストと実行器の最後に出る「エネルギー帳尻の誤差」が 0 付近であること。
  詳しくは [PLAN.md](PLAN.md) の「エネルギーを湧かせないためのルール」
- **決定論**: 同じシード・同じパラメータなら同じ歴史になる。並列でも同じ。保存して読み込んだ続きも、保存せずに進めた場合と同じ
  （`save.rs` のテスト）
- **二重の死を作らない**: 1 tick の中で複数の理由で死にうる。`die()` は死んでいる個体には何もしない。
  死んだ個体の体のエネルギーは死体として 1 回だけ落とす

## 6. 保存形式

- 中身: `"LSIMWRLD"` ＋ 形式の番号（`FORMAT_VERSION`）＋ 構造の見出し（形質の数・入力の数・出力の数・中間 1・中間 2・遺伝子の長さ・死因の数）＋ 世界 ＋ 長期の統計の記録
- 長期の統計の記録は末尾に、バイト数を先に書いて置く（形式 3 から）。記録の並び（`KIND_LEN`）が今のコードと違えば読み飛ばし、記録を空から始める。
  記録は世界の進み方に関わらないので、なくても歴史は同じに続く。形式 2（記録なし）も読める
- 入れ物: `"LIFESIM\n"` ＋ 見出しの長さ（u32）＋ JSON の見出し（形式、コミット、保存した日時、tick、個体数）＋ 中身。
  ブラウザは gzip で圧縮して `.lifesim.gz`、画面なしの実行器は圧縮なしの `.lifesim` を書く。ブラウザはどちらも読める
- 構造の見出しが違えば読み込みを断る。個体の持つ値の並びを変えたら `FORMAT_VERSION` を上げる。
  **互換性は保たない方針**（ユーザーの了承済み）。古い保存ファイルの移行は書かない
- 知らないパラメータは無視して読み込む（パラメータの追加では保存ファイルは壊れない）

## 7. ビルドと実行

```sh
npm run dev          # WASM をビルドして Vite を起動
npm run build:sim    # WASM だけ作り直す（Rust を変えたら必ず。開発サーバーは止めなくていいが、ブラウザは再読み込み）
npm test             # cargo test
npm run typecheck    # tsc
npm run build        # dist/ に出力
```

画面なしの実行（`--features parallel` を付けると判断の段を並列化する。結果は同じ）:

```sh
cargo run --release --features parallel --example headless --manifest-path sim/Cargo.toml -- 120000 seed=3 every=8000
cargo run --release --features parallel --example diagnose --manifest-path sim/Cargo.toml -- 120000 seed=3 every=8000
```

複数のシードを同時に回すときは、`CARGO_TARGET_DIR=sim/target-a` のようにビルド先を分ける（同じビルド先だとロックの待ちになる）。
`sim/target-*/` は .gitignore 済み。

回し方の速さ（16 スレッドの PC、1 万 tick、2026-11-17 計測）:

| 回し方 | 全体の時間 | 1 本あたり |
|---|---|---|
| 並列あり 1 本 | 41 秒 | 41 秒 |
| 並列あり 3 本同時 | 96 秒 | 32 秒 |
| 並列なし 1 本 | 144 秒 | 144 秒 |
| 並列なし 8 本同時 | 344 秒 | 43 秒 |
| 並列なし 16 本同時 | 505 秒 | 32 秒 |

シード 1〜3 の検証は、並列ありの 3 本同時がいちばん早い。並列なしで本数を増やしても、1 本あたりは同じ 32 秒より速くならない（メモリの帯域で頭打ち）。

1 tick の段ごとの時間は `--features parallel,profile` で測れる（実行の最後に表示する）。並列ありでは、順番に処理する行動の段が約半分を占める。
判断の段（知覚と脳）は並列で 9 倍速くなるので、これ以上速くするには行動の段（1 体あたり約 130 ns）を詰める必要がある

### 変更を入れるときの検証の手順

生態系の振る舞いを変える変更は、入れる前に画面なしで確かめる。

1. 既定値と変更後で、シード 1・2・3 を 12 万 tick 回す（肉食の崩れは 9〜11 万 tick ごろに出ることが多いので、短いと見逃す）
2. 見るもの: 肉食と草食の個体数の推移（絶滅・爆発がないか）、体の大きさの平均とばらつき、狩りの成功率、獲物 1 体から得る量、死因の内訳
3. 結果を [PLAN.md](PLAN.md) に日付付きで記録し、仕様が変わったら [ECOSYSTEM.md](ECOSYSTEM.md) を直す
4. ブラウザでも開いて、表示が壊れていないか（NaN が出ていないか）を見る

## 8. 落とし穴

- **Rust を変えたのに `npm run build:sim` を忘れる**: 画面は古い WASM のまま動き、並びの長さがずれて NaN が出る。
  Vite は WASM の変更を拾うが、Worker を作り直すにはブラウザの再読み込みが要ることがある
- **params.rs だけ既定値を変える**: ブラウザは config.ts の値を使うので反映されない
- **判断の段で状態を書き換える・共有の乱数を引く**: 並列の結果が変わり、決定論のテストが落ちる
- **保存ファイルの互換**: 脳や遺伝子の構造を変えると古い保存ファイルは読めない（意図どおり）。検証用の .lifesim は git に入れない
- **初期の脳の重み**: 初期の個体は進化前なので、行動の起点になる重み（攻撃の偏り、繁殖の欲求など）が強すぎても弱すぎても、
  その行動が一度も起きず選択が働かない。新しい行動は「たまに起きる」くらいの初期値にし、創始個体ごとにばらつかせる
- **Windows の Git Bash**: パスは `/c/Users/...`。PowerShell でのプロセスの調べ方は自動許可されないことがある（tasklist / taskkill を使う）

## 9. コミットと公開

- コミットメッセージは日本語、`種別: 要約`（feat / fix / refactor / docs / chore）。仕様書の更新は実装と別のコミットにする
- main に push すると GitHub Actions がビルドして公開する（<https://karuru6225.github.io/life-sim/>）。push は明示的に頼まれたときだけ

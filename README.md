# life-sim

数万体の生き物が暮らす 2D の生態系シミュレーション。ブラウザで動き、観察して遊ぶ。

**公開ページ: https://karuru6225.github.io/life-sim/**

植物・草食・肉食の 3 つの栄養段階があり、エネルギーは日射から一方通行で流れる。個体数に上限はなく、日射の量だけで決まる。
個体は遺伝子で体の形質と脳（小さなニューラルネット）が決まり、生き残って繁殖した個体の遺伝子が変異しながら受け継がれる。
群れ、季節移動、繁殖期、種の分かれ始めなどが、決め打ちではなく進化の結果として現れるかを観察できる。

## 遊び方

- ドラッグで移動、ホイールでズーム、ダブルクリックで全体表示、Space で一時停止
- 拡大して個体をクリックすると、空腹度・欲求・遺伝子・生涯の記録・脳の入出力などが見られる
- 左のパネルで、色分け（種類／系統色）、強調表示、地図（植生／気温）、速度、日射量を切り替えられる
- 系統色のヒストグラムの棒をクリックすると、その系統の統計を見られる
- 「保存」で世界をファイルに保存し、「読み込み」で続きから再開できる

## 文書

- [docs/ECOSYSTEM.md](docs/ECOSYSTEM.md): 生態系の仕様（どんな決まりで動いているか）
- [docs/PLAN.md](docs/PLAN.md): 設計と開発の記録、検証結果、今後の計画

## 技術

- シミュレーション: Rust → WebAssembly（`wasm32-unknown-unknown`、wasm-bindgen なし）。Web Worker で動かす
- 描画: PixiJS v8（ParticleContainer）
- ビルド: Vite、TypeScript

## 手元で動かす

Rust（`wasm32-unknown-unknown` ターゲット）と Node.js が必要。

```sh
rustup target add wasm32-unknown-unknown
npm install
npm run dev      # WASM をビルドして開発サーバーを起動
npm test         # シミュレーションのテスト（cargo test）
npm run build    # dist/ に静的ファイルを出力
```

画面なしで長期の推移を調べる実行器もある（`sim/examples/`）。`--features parallel` を付けると、知覚と脳の判断を
複数のスレッドで並列に計算する（結果は付けない場合と同じ。16 コアで約 5 倍速い）。

```sh
cargo run --release --features parallel --example headless --manifest-path sim/Cargo.toml -- 30000 seed=3 every=1000
```

`save=ファイル名` を付けると、最後にブラウザで読み込める保存ファイルを書き出す。画面なしで速く進めて、続きをブラウザで見られる。

```sh
cargo run --release --features parallel --example headless --manifest-path sim/Cargo.toml -- 50000 seed=3 save=world.lifesim
# ブラウザの「読み込み」で world.lifesim を選ぶ
```

書き出したファイルから、画面なしで続きを進めることもできる（`resume`）。

```sh
cargo run --release --features parallel --example resume --manifest-path sim/Cargo.toml -- world.lifesim 10000 save=続き.lifesim
```

main に push すると、GitHub Actions がビルドして GitHub Pages に公開する（`.github/workflows/pages.yml`）。

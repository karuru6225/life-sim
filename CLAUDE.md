# life-sim

数万体の生き物の 2D 生態系シミュレーション（Rust → WASM ＋ PixiJS）。進化の結果として群れ・季節移動・種分化が現れるかを観察する。

作業を始める前に読む:

- [docs/HANDOVER.md](docs/HANDOVER.md): 現状、未解決の問題、次の作業、ユーザーと合意した方針
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): コードの構成、変更の手順、検証の手順、落とし穴
- [docs/ECOSYSTEM.md](docs/ECOSYSTEM.md): 生態系の仕様（挙動を変えたら必ず更新する）
- [docs/PLAN.md](docs/PLAN.md): 設計の経緯と検証の記録（検証したら日付付きで追記する）

## 要点

- Rust を変えたら `npm run build:sim`（忘れると画面が古い WASM のまま動き、NaN が出る）
- パラメータの既定値は `sim/src/params.rs` と `web/src/config.ts` の両方にある。変えるときは両方
- 判断の段（`plan`）では世界を書き換えない・乱数を引かない。並列でも順番でも同じ結果になること
- エネルギーは帳簿（`ledger`）に記録して動かす。湧かせない
- 生態系を変える変更は、画面なしの実行器でシード 1〜3 を 12 万 tick 回して確かめてから入れる
- 挙動を直接書き込まず、損得の仕組みを整えて進化に任せる
- push は指示があるときだけ

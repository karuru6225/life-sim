// Rust のビルド成果物を web/src/wasm/ にコピーする（Vite から ?url で読み込むため）
import { copyFileSync, mkdirSync } from 'node:fs';

const from = 'sim/target/wasm32-unknown-unknown/release/sim.wasm';
const to = 'web/src/wasm/sim.wasm';

mkdirSync('web/src/wasm', { recursive: true });
copyFileSync(from, to);
console.log(`${from} -> ${to}`);

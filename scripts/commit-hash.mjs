// ビルドしたコードのコミット（短いハッシュ）を返す。コミットしていない変更があれば -dirty を付ける。git がなければ unknown。
// vite.config.ts から使う（Node の型定義を入れずに済むよう、JS で書いている）
import { execSync } from 'node:child_process';

export function commitHash() {
  try {
    const hash = execSync('git rev-parse --short HEAD').toString().trim();
    const dirty = execSync('git status --porcelain').toString().trim() !== '';
    return dirty ? `${hash}-dirty` : hash;
  } catch {
    return 'unknown';
  }
}

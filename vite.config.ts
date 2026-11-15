import { defineConfig } from 'vite';
import { commitHash } from './scripts/commit-hash.mjs';

export default defineConfig({
  root: 'web',
  // 静的ホスティングのどのパスに置いても動くように相対パスで出力する
  base: './',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
  },
  worker: {
    format: 'es',
  },
  // 保存した世界のファイルに、どの版で保存したかを書くため（開発サーバーでは起動したときの値）
  define: {
    __COMMIT__: JSON.stringify(commitHash()),
  },
});

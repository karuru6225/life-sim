import { defineConfig } from 'vite';

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
});

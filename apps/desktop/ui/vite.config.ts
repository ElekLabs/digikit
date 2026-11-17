// The desktop app's own frontend (a developer console). It shares the panel, the pages and the native runtime with the
// site through the package @digi/workbench-core.
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  root: here('.'),
  base: './',
  plugins: [solid()],
  resolve: { dedupe: ['solid-js'] },
  build: { outDir: 'dist', emptyOutDir: true, target: 'safari16' },
  server: { fs: { allow: [here('../../..')] } },
});

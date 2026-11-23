// Serve the desktop entry with the preview environment and instrument fixtures.
import { fileURLToPath } from 'node:url';
import { mergeConfig } from 'vite';
import desktop from '../vite.config.ts';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

export default mergeConfig(desktop, {
  optimizeDeps: { include: ['fft.js', 'pixi.js', 'pixi.js/unsafe-eval'] },
  resolve: {
    alias: [
      { find: './workbench-environment', replacement: here('./main.tsx') },
      { find: '@tauri-apps/api/core', replacement: here('./tauri-mock.ts') },
      { find: /^\.\/memory-backend$/, replacement: here('./memory-backend.mock.ts') },
      { find: /^\.\/calibration-backend$/, replacement: here('./calibration-backend.mock.ts') },
      { find: /^\.\/recorder$/, replacement: here('./profile-backend.mock.ts') },
      { find: /^\.\/samples-backend$/, replacement: here('./samples-backend.mock.ts') },
    ],
  },
  server: { host: '127.0.0.1', port: 5199, strictPort: true },
});

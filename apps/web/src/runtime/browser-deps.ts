// What the site's pages give `browserRuntime` (runtime.ts): the browser's workers, and the emulator core fetched from the site's base URL.
import { fetchWasmSource } from '@digi/workbench-core/wasm-source.ts';
import { browserWorkers } from '../browser-workers';
import type { BrowserRuntimeDeps } from '@digi/workbench-core/runtime/runtime.ts';

const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');

export const browserRuntimeDeps = (): BrowserRuntimeDeps => ({
  workers: browserWorkers(),
  wasm: fetchWasmSource(base),
});

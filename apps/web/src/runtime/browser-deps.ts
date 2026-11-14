// What the site's pages give `browserRuntime` (runtime.ts): the browser's workers, and the emulator core fetched from the site's base URL.
import { fetchWasmSource } from '../wasm-source';
import { browserWorkers } from '../workers';
import type { BrowserRuntimeDeps } from './runtime';

const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');

export const browserRuntimeDeps = (): BrowserRuntimeDeps => ({
  workers: browserWorkers(),
  wasm: fetchWasmSource(base),
});

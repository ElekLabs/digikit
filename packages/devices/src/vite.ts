// Vite loader: the device files are bundled as text (`import.meta.glob`), so a browser build needs no file system.
import { parse, type Registry } from './parse.ts';

export * from './index.ts';

declare global {
  interface ImportMeta {
    glob(pattern: string, options: { query: string; import: string; eager: true }): Record<string, string>;
  }
}

const raw = import.meta.glob('../../../devices/*.toml', { query: '?raw', import: 'default', eager: true });

/** The registry of the repository's device files. */
export const registry: Registry = parse(
  Object.fromEntries(Object.entries(raw).map(([p, text]) => [p.slice(p.lastIndexOf('/') + 1), text])),
);

// Vite loader: the device files are bundled as text (`import.meta.glob`), so a browser build needs no file system.
import { parse, type Registry } from './parse.ts';

export * from './index.ts';

// `import.meta.glob` is Vite's; the apps that bundle this file have its types, this package does not (no Vite dependency).
// @ts-ignore
const raw: Record<string, string> = import.meta.glob('../../../devices/*.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** The registry of the repository's device files. */
export const registry: Registry = parse(
  Object.fromEntries(Object.entries(raw).map(([p, text]) => [p.slice(p.lastIndexOf('/') + 1), text])),
);

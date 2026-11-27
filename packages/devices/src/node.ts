// Node loader: reads `devices/*.toml` of the repository (or of `dir`).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, type Registry } from './parse.ts';

export * from './index.ts';

const repoDevices = fileURLToPath(new URL('../../../devices', import.meta.url));

/** The registry of the device files in `dir` (default: `devices/` of the repository). */
export function loadDevices(dir: string = repoDevices): Registry {
  const files: Record<string, string> = {};
  for (const name of readdirSync(dir).filter((n) => n.endsWith('.toml')))
    files[name] = readFileSync(join(dir, name), 'utf8');
  return parse(files);
}

let cached: Registry | undefined;
/** The registry of the repository's device files, read once. */
export function registry(): Registry {
  return (cached ??= loadDevices());
}

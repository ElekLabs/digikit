// The ready-inputs directory of a device (repo-relative): `[acceptance] ready_dir` of its device file, the one source (docs/design/device-data.md).
// The Python tools (tools/emulator/digi_env.py) and the TypeScript tests (packages/test-support/src/inputs.ts) read the same key.
import { globSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/** `{ short: ready_dir }` of every device file with an `[acceptance]` table. */
export const readyDirs = Object.fromEntries(
  globSync('devices/*.toml', { cwd: root })
    .map((f) => parse(readFileSync(join(root, f), 'utf8')))
    .filter((t) => t.device?.short && t.acceptance?.ready_dir)
    .map((t) => [t.device.short, t.acceptance.ready_dir]),
);

// The files of the devices that have no acceptance cell (no `[acceptance] ready_dir`, see ready-dirs.mjs): no device test runs on them.
// Device-test keys leave them out (device-test-keys.mjs): an edit of them cannot change a DT2 or DN2 run.
import { globSync, readFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/** Repository-relative globs of the device file, its `<stem>-*` companions (LED map, peer file) and its ports, per device without a cell. */
export const nonCellGlobs = (workspaceRoot = root) =>
  globSync('devices/*.toml', { cwd: workspaceRoot })
    .sort()
    .flatMap((file) => {
      const t = parse(readFileSync(join(workspaceRoot, file), 'utf8'));
      if (!t.device?.short || t.acceptance?.ready_dir) return [];
      const stem = basename(file, '.toml');
      return [`devices/${stem}.toml`, `devices/**/${stem}-*.toml`, `devices/ports/${t.device.short}-*.toml`];
    });

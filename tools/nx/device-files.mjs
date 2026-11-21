// The files of each device: its device file, its `<stem>-*` companions (LED map, peer file) and its ports. A device with no acceptance cell
// (no `[acceptance] ready_dir`, see ready-dirs.mjs) runs no device test. Device-test keys leave out the files of every other device
// (device-test-keys.mjs, patchkit-key.mjs): an edit of them cannot change a run on this device.
import { globSync, readFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const devices = (workspaceRoot) =>
  globSync('devices/*.toml', { cwd: workspaceRoot })
    .sort()
    .flatMap((file) => {
      const t = parse(readFileSync(join(workspaceRoot, file), 'utf8'));
      if (!t.device?.short) return [];
      return [{ short: t.device.short, stem: basename(file, '.toml'), cell: Boolean(t.acceptance?.ready_dir) }];
    });
const globsOf = ({ stem, short }) => [`devices/${stem}.toml`, `devices/**/${stem}-*.toml`, `ports/${short}-*.toml`];

/** The short ids of the device files. */
export const deviceShorts = (workspaceRoot = root) => devices(workspaceRoot).map((d) => d.short);
/** The short ids of the devices with an acceptance cell: the ones a device test runs on. */
export const cellShorts = (workspaceRoot = root) =>
  devices(workspaceRoot)
    .filter((d) => d.cell)
    .map((d) => d.short);
/** Repository-relative globs of the files of the device `short`. */
export const filesOf = (short, workspaceRoot = root) => {
  const own = devices(workspaceRoot).find((d) => d.short === short);
  if (!own) throw new Error(`no device file with short = "${short}"`);
  return globsOf(own);
};
/** Globs of the files of every device except `short`. */
export const otherFiles = (short, workspaceRoot = root) =>
  devices(workspaceRoot)
    .filter((d) => d.short !== short)
    .flatMap(globsOf);
/** Globs of the files of the devices without a cell: no device test runs on them. */
export const nonCellGlobs = (workspaceRoot = root) =>
  devices(workspaceRoot)
    .filter((d) => !d.cell)
    .flatMap(globsOf);

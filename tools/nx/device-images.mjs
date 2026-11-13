// Writes dist/bytes/images.sha256: `<device>-<version> <set> image <sha256> plan <sha256> addrmap <sha256>` for the image, the plan and the address map that `patchkit apply` makes from each
// acceptance stock firmware (CELLS of device-inputs.mjs) with each patch set of its device profile (devices/*.toml). This is the
// code path the daemon takes at boot (patchkit Library, Registry and apply::apply). The device tasks of a merge key hash this file
// and not the patchkit binary: a patchkit edit that leaves every packed image byte alone changes no key. A missing firmware
// file writes no line for it (the tasks hash the firmware itself in device-inputs.mjs).
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash } from 'node:crypto';
import { existsSync, globSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { CELLS } from './device-inputs.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const pk = join(root, 'dist/patchkit/patchkit');
const setsOf = new Map(
  globSync('devices/*.toml', { cwd: root })
    .map((f) => parse(readFileSync(join(root, f), 'utf8')))
    .filter((t) => t.device?.short)
    .map((t) => [t.device.short, Object.keys(t.patch_sets ?? {}).sort()]),
);
const scratch = mkdtempSync(join(tmpdir(), 'digi-images-'));
const run = promisify(execFile);
const jobs = CELLS.filter((cell) => existsSync(join(root, cell.syx))).flatMap((cell) =>
  (setsOf.get(cell.dev) ?? []).map((set) => ({ cell, set })),
);
// The jobs only read; four at a time keep the machine usable. Lines keep the order of CELLS and the sets.
const lines = Array.from({ length: jobs.length }, () => '');
let next = 0;
try {
  await Promise.all(
    Array.from({ length: 4 }, async () => {
      while (next < jobs.length) {
        const index = next++;
        const { cell, set } = jobs[index];
        const out = join(scratch, `${cell.dev}-${set}.syx`);
        const plan = out.replace(/\.syx$/, '.plan.json');
        const addrmap = out.replace(/\.syx$/, '.addrmap.json');
        // the plan and the address map are what tests/regression/runtime/runtime-plan.ts reads (offline image preparation)
        const args = [
          '--root',
          root,
          'apply',
          '--firmware',
          cell.syx,
          '--set',
          set,
          '-o',
          out,
          '--plan',
          plan,
          '--addrmap',
          addrmap,
        ];
        await run(pk, args, { cwd: root });
        const sha = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
        lines[index] = `${cell.dev}-${cell.ver} ${set} image ${sha(out)} plan ${sha(plan)} addrmap ${sha(addrmap)}`;
      }
    }),
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
mkdirSync(join(root, 'dist/bytes'), { recursive: true });
writeFileSync(join(root, 'dist/bytes/images.sha256'), `${lines.join('\n')}\n`);

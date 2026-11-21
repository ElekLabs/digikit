// Patch sets and static acceptance root sets used by the image producer and the E8 manifest.
import { globSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'smol-toml';
import { CELLS } from './device-inputs.mjs';

export const imageJobs = (root) => {
  const setsOf = new Map(
    globSync('devices/*.toml', { cwd: root })
      .map((f) => parse(readFileSync(join(root, f), 'utf8')))
      .filter((t) => t.device?.short)
      .map((t) => [t.device.short, Object.keys(t.patch_sets ?? {}).sort()]),
  );
  // The static --modules sets are also read by tests/regression/runtime/runtime-plan.ts.
  const { sets } = JSON.parse(readFileSync(join(root, 'tests/regression/runtime/image-roots.json'), 'utf8'));
  return CELLS.flatMap((cell) => [
    ...(setsOf.get(cell.dev) ?? []).map((set) => ({ cell, label: set, select: ['--set', set] })),
    ...sets
      .filter((set) => !set.devices || set.devices.includes(cell.dev))
      .map((set) => ({
        cell,
        label: `roots:${set.id}`,
        select: ['--modules', set.roots.join(',')],
        keep: join(root, 'dist/bytes/images', cell.dev, set.id),
      })),
  ]);
};

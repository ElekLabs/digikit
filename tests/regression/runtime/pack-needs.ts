import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, type Device } from '@digi/test-support';
import { fxExpectationsWith, GAIN_PAGE_DEFAULTS, type FxExpectations } from './dkm-parse.ts';
import { moduleArtifact } from './install.ts';

/**
 * The `patchkit needs` verdict of a committed pack, read from dist/bytes/pack-verdicts.json (written by `nx run patchkit:pack-verdicts`
 * from the packs in needs-packs.json). This file never runs patchkit, so a test that only needs this does not count as running it live:
 * its key hashes the verdict file. A pack or a changed pack without an entry fails here and never falls back to the binary.
 */
const listed = (): string[] =>
  (
    JSON.parse(fs.readFileSync(path.join(repoRoot(), 'tests/regression/runtime/needs-packs.json'), 'utf8')) as {
      modules: string[];
    }
  ).modules;

export function committedNeeds(device: Device, id: string): { file: string; needs: Record<string, number> } {
  if (!listed().includes(id))
    throw new Error(`${id} is not in tests/regression/runtime/needs-packs.json: add it, or use needsOf (dkm.ts)`);
  const file = moduleArtifact(device, id);
  const key = `needs:${createHash('sha256').update(fs.readFileSync(file)).digest('hex')}`;
  const verdicts = JSON.parse(
    fs.readFileSync(path.join(repoRoot(), 'dist/bytes/pack-verdicts.json'), 'utf8'),
  ) as Record<string, Record<string, number>>;
  const needs = verdicts[key];
  if (!needs) throw new Error(`no precomputed needs for ${file} (${key}): run nx run patchkit:pack-verdicts`);
  return { file, needs };
}

/** Relocation and row oracle of the one-effect pack `id` placed at BASE (the gain fixtures). */
export function committedFxExpectations(
  device: Device,
  id: string,
  base: number,
  pageDefaults = GAIN_PAGE_DEFAULTS,
): FxExpectations {
  const { file, needs } = committedNeeds(device, id);
  return fxExpectationsWith(file, base, needs, pageDefaults);
}

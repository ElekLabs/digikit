import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { repoRoot } from '@digi/test-support';
import { patchkit } from './patchkit.ts';

/** One text edit of a source file of the module: `from` must occur exactly once. */
export interface SourceEdit {
  file: string;
  from: string;
  to: string;
}

/** An edit of the DSP source of `test/sine` (`tests/fixtures/modules/sine`) that raises its pitch by an octave: the phase increment constant `440 * 2^(-69/12) / 48000` doubles (exponent + 1). */
export const SINE_OCTAVE_UP: SourceEdit = { file: 'src/dsp/sine.s', from: 'R1 = 0x39329a62;', to: 'R1 = 0x39b29a62;' };

/**
 * Pack `id` for `device` from a copy of its module directory in which `edits` are applied, so a test gets two builds with the same module id and keys that differ in a constant (a reload
 * between them keeps the old module's keys; a new fixture identity is not needed). The copy lives in a temporary directory and is named by `--manifest`, which replaces the module
 * search of patchkit for that run: the committed objects and the cache of `out/cache/dko` are not touched. Returns the path of the packed file, `out`.
 */
export async function packModuleVariant(opts: {
  /** The module directory from the repository root, for example `tests/fixtures/modules/sine`. */
  moduleDir: string;
  id: string;
  device: 'dt2' | 'dn2';
  edits: SourceEdit[];
  out: string;
}): Promise<string> {
  const from = path.join(repoRoot(), opts.moduleDir);
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-variant-'));
  const copy = path.join(work, path.basename(from));
  // the module's sources and its generated headers (`build/gen`); its own tests and objects stay behind
  // the filter sees paths below the module directory: the fixture itself lives under `tests/fixtures/`
  fs.cpSync(from, copy, {
    recursive: true,
    filter: (src) => !/(^|[\\/])tests([\\/]|$)/.test(path.relative(from, src)),
  });
  for (const e of opts.edits) {
    const file = path.join(copy, e.file);
    const text = fs.readFileSync(file, 'utf8');
    const n = text.split(e.from).length - 1;
    if (n !== 1)
      throw new Error(`${opts.moduleDir}/${e.file}: ${JSON.stringify(e.from)} occurs ${n} times, expected once`);
    fs.writeFileSync(
      file,
      text.replace(e.from, () => e.to),
    );
  }
  fs.mkdirSync(path.dirname(opts.out), { recursive: true });
  await patchkit(
    ['pack', opts.id, '--device', opts.device, '--manifest', path.join(copy, 'module.toml'), '-o', opts.out],
    { timeout: 240_000 },
  );
  return opts.out;
}

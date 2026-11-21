import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { expect, it } from 'vitest';
import { trackedObjects } from '../tracked-objects.mjs';

it('finds indexed objects by the owning manifest after a core module leaves modules/', () => {
  const root = mkdtempSync(join(tmpdir(), 'digikit-tracked-objects-'));
  const git = (...args: string[]) => execFileSync('git', args, { cwd: root });
  const write = (path: string, bytes: string) => {
    mkdirSync(join(root, dirname(path)), { recursive: true });
    writeFileSync(join(root, path), bytes);
  };
  try {
    git('init', '--quiet');
    const from = 'modules/core/example';
    const to = 'runtime/platform/example';
    const fixture = 'fixtures/example';
    for (const [dir, category] of [
      [from, 'core'],
      [fixture, 'test'],
    ]) {
      write(`${dir}/module.toml`, `[module]\ncategory = "${category}"\n`);
      write(`${dir}/build/example.dko.json`, '{}');
    }
    git('add', from, fixture);
    mkdirSync(join(root, dirname(to)), { recursive: true });
    git('mv', from, to);
    write(`${to}/build/local.dko.json`, '{}');
    expect(Object.fromEntries(trackedObjects(root))).toEqual({
      [`${to}/build/example.dko.json`]: { dir: to, category: 'core' },
      [`${fixture}/build/example.dko.json`]: { dir: fixture, category: 'test' },
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

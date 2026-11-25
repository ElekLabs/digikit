import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { expect, it } from 'vitest';
import { walkFiles } from '../tree.mjs';

it('lists the regular files at every depth, skips directories and links, and names nothing for a missing directory', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tree-'));
  try {
    mkdirSync(join(dir, 'a/b'), { recursive: true });
    mkdirSync(join(dir, 'empty'));
    writeFileSync(join(dir, 'top'), '');
    writeFileSync(join(dir, 'a/b/deep'), '');
    symlinkSync(join(dir, 'top'), join(dir, 'link'));
    expect(
      walkFiles(dir)
        .map((p) => relative(dir, p))
        .sort(),
    ).toEqual(['a/b/deep', 'top']);
    expect(walkFiles(join(dir, 'absent'))).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

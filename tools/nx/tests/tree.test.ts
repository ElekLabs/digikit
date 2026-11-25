import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { expect, it } from 'vitest';
import { walkFiles } from '../tree.mjs';

const rel = (dir: string, files: string[]) => files.map((p) => relative(dir, p)).sort();

it('lists the regular files at every depth and skips directories and links', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tree-'));
  try {
    mkdirSync(join(dir, 'a/b'), { recursive: true });
    mkdirSync(join(dir, 'empty'));
    writeFileSync(join(dir, 'top'), '');
    writeFileSync(join(dir, 'a/b/deep'), '');
    symlinkSync(join(dir, 'top'), join(dir, 'link'));
    expect(rel(dir, walkFiles(dir))).toEqual(['a/b/deep', 'top']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('skips an unreadable directory and keeps the rest, unless strict', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tree-'));
  try {
    mkdirSync(join(dir, 'locked'));
    writeFileSync(join(dir, 'locked/hidden'), '');
    writeFileSync(join(dir, 'seen'), '');
    chmodSync(join(dir, 'locked'), 0);
    expect(rel(dir, walkFiles(dir))).toEqual(['seen']);
    expect(walkFiles(join(dir, 'absent'))).toEqual([]);
    expect(() => walkFiles(dir, { strict: true })).toThrow();
    expect(() => walkFiles(join(dir, 'absent'), { strict: true })).toThrow();
  } finally {
    chmodSync(join(dir, 'locked'), 0o755);
    rmSync(dir, { recursive: true, force: true });
  }
});

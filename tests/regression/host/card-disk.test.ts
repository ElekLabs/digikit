import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { allocatedBytes, cloneFile } from '@digi/test-support';
import { afterEach, describe, expect, test } from 'vitest';
import { buildCard, plusdriveBinary } from '../runtime/card-kit.ts';
import { applyOverlay } from '../runtime/write-safety.ts';

/** A card file is about 25 GB of address space and a few MB of data: copying one must not allocate the holes. */
const FULL_CARD = 24_750_587_904;
const LIMIT = 64 << 20;

const directories: string[] = [];
const tmp = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-card-disk-'));
  directories.push(dir);
  return dir;
};
afterEach(() => {
  for (const dir of directories.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function sparseCard(file: string): void {
  const fd = fs.openSync(file, 'w');
  try {
    fs.ftruncateSync(fd, FULL_CARD);
    fs.writeSync(fd, Buffer.alloc(512, 7), 0, 512, 4096);
  } finally {
    fs.closeSync(fd);
  }
}

const freeBytes = (dir: string) => {
  const s = fs.statfsSync(dir);
  return s.bavail * s.bsize;
};

describe('card images stay sparse', () => {
  test('cloneFile of a full-size card allocates almost nothing, keeps the data and does not share later writes', () => {
    const dir = tmp();
    const base = path.join(dir, 'base.img');
    sparseCard(base);
    const before = freeBytes(dir);
    const copy = path.join(dir, 'copy.img');
    cloneFile(base, copy);
    expect(fs.statSync(copy).size).toBe(FULL_CARD);
    expect(allocatedBytes(copy)).toBeLessThan(LIMIT);
    expect(before - freeBytes(dir)).toBeLessThan(LIMIT);
    const fd = fs.openSync(copy, 'r+');
    fs.writeSync(fd, Buffer.alloc(512, 9), 0, 512, 4096);
    fs.closeSync(fd);
    const read = (file: string) => {
      const fd = fs.openSync(file, 'r');
      const b = Buffer.alloc(512);
      fs.readSync(fd, b, 0, 512, 4096);
      fs.closeSync(fd);
      return b[0];
    };
    expect([read(base), read(copy)]).toEqual([7, 9]);
  });

  test('buildCard over a full-size base and applyOverlay allocate almost nothing', async () => {
    const dir = tmp();
    const base = path.join(dir, 'base.img');
    sparseCard(base);
    const before = freeBytes(dir);
    const card = await buildCard('dn2', { dir, base, selection: ['modules/a.dkm'] });
    const overlay = path.join(dir, 'o.bin');
    const record = Buffer.alloc(8 + 64 + 512);
    record.writeBigUInt64LE(5n);
    record.fill(0xff, 8, 72);
    fs.writeFileSync(overlay, record);
    const after = path.join(dir, 'after.img');
    expect(applyOverlay(card, overlay, after)).toBe(1);
    expect(fs.statSync(after).size).toBe(FULL_CARD);
    expect(allocatedBytes(card)).toBeLessThan(LIMIT);
    expect(allocatedBytes(after)).toBeLessThan(LIMIT);
    expect(before - freeBytes(dir)).toBeLessThan(LIMIT);
  });

  test.skipIf(!fs.existsSync(plusdriveBinary()))('a card built by plusdrive is sparse', async () => {
    const dir = tmp();
    const before = freeBytes(dir);
    const card = await buildCard('dn2', { dir, selection: [] });
    expect(allocatedBytes(card)).toBeLessThan(LIMIT);
    expect(before - freeBytes(dir)).toBeLessThan(LIMIT);
  });
});

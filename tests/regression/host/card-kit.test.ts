import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, test } from 'vitest';
import { LOAD_STATE } from '@digi/dk-session/gen_proto';
import { cardStore } from '../runtime/device-data.ts';
import {
  bootInfoFinal,
  buildCard,
  currentRecord,
  decodeRecord,
  encodeRecord,
  plusdriveBinary,
  readSectors,
  recordsIn,
} from '../runtime/card-kit.ts';
import { SYSTEM_PACKAGE_STORE_PATH, systemPackageFile } from '../runtime/system-package.ts';
import { cardAfter, encodeOverlay, linkCard } from '../runtime/write-safety.ts';

const device = 'dn2';
const directories: string[] = [];
const tmp = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-card-kit-'));
  directories.push(dir);
  return dir;
};
afterEach(() => {
  for (const dir of directories.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe('selection record', () => {
  test('round trips, with the sticky map at its two offsets', () => {
    const map = [0x11, 0, 0, 0, 0, 0, 0, 0x77];
    const rec = { paths: ['modules/a.dkm', 'modules/b.dkm'], seq: 7, counter: 2, flags: 1, stableBlocks: 300, map };
    const bytes = encodeRecord(rec);
    expect(bytes.subarray(0, 4).toString()).toBe('DKSL');
    expect(decodeRecord(bytes)).toEqual(rec);
    expect(bytes.readUInt32BE(28)).toBe(0x11);
    expect(bytes.readUInt32BE(504)).toBe(0x77);
  });

  test('a torn copy and a bad count do not decode; the higher seq wins and A wins a tie', () => {
    const bytes = encodeRecord({ paths: ['x'] });
    const torn = Buffer.from(bytes);
    torn.writeUInt8(torn.readUInt8(40) ^ 1, 40);
    expect(decodeRecord(torn)).toBeNull();
    expect(decodeRecord(Buffer.alloc(512))).toBeNull();
    expect(() => encodeRecord({ paths: Array(9).fill('x') })).toThrow();
    const [a, b] = [
      decodeRecord(encodeRecord({ paths: ['a'], seq: 2 })),
      decodeRecord(encodeRecord({ paths: ['b'], seq: 3 })),
    ];
    expect(currentRecord([a, b])).toBe(b);
    expect(currentRecord([a, a])).toBe(a);
    expect(currentRecord([null, b])).toBe(b);
  });
});

describe('card sectors', () => {
  const { selA, selB } = cardStore(device);

  test('buildCard over a base places the copies, a torn copy and foreign sectors; the overlay wins over the image', async () => {
    const dir = tmp();
    const base = path.join(dir, 'base.img');
    fs.writeFileSync(base, Buffer.alloc(512));
    const foreign = Buffer.alloc(512, 0xab);
    const card = await buildCard(device, {
      dir,
      base,
      selection: ['modules/a.dkm'],
      counter: 3,
      recs: [null, 'torn'],
      foreign: { 5: foreign },
    });
    expect(recordsIn(card, device)).toEqual([null, null]);
    expect(readSectors(card, 5, 1).equals(foreign)).toBe(true);
    const both = await buildCard(device, { dir, name: 'both', base, selection: ['modules/a.dkm'], counter: 3 });
    expect(recordsIn(both, device).map((r) => r?.counter)).toEqual([3, 3]);
    // a guest write over copy B: bytes 0..3 (the magic) cleared by the mask
    const overlay = Buffer.alloc(8 + 64 + 512);
    overlay.writeBigUInt64LE(BigInt(selB));
    overlay[8] = 0x0f;
    expect(recordsIn(both, device, overlay).map((r) => r?.counter)).toEqual([3, undefined]);
    // base + overlay reads equal the bytes written, and a card after the guest's writes keeps the earlier ones
    const written = path.join(dir, 'written.bin');
    fs.writeFileSync(written, encodeOverlay(new Map([[5, Buffer.alloc(512, 0xcd)]])));
    const after = cardAfter(both, written, path.join(dir, 'after'));
    expect(readSectors(after, 5, 1).equals(Buffer.alloc(512, 0xcd))).toBe(true);
    expect(recordsIn(after, device).map((r) => r?.counter)).toEqual([3, 3]);
    expect(recordsIn(both, device).map((r) => r?.counter)).toEqual([3, 3]);
    expect(readSectors(both, 5, 1).equals(Buffer.alloc(512))).toBe(true);
    expect(selA).toBeLessThan(selB);
  });

  test.skipIf(!fs.existsSync(plusdriveBinary()))(
    'the plusdrive builder writes the bytes encodeRecord writes',
    async () => {
      const dir = tmp();
      const modules = [{ name: 'modules/a.dkm', file: path.join(dir, 'a.dkm') }];
      fs.writeFileSync(modules[0]!.file, Buffer.alloc(64));
      const card = await buildCard(device, { dir, modules });
      fs.writeFileSync(path.join(dir, 'seed-card', 'digikit', 'selection.txt'), 'modules/a.dkm\n');
      const cli = path.join(dir, 'cli.img');
      const { execFileSync } = await import('node:child_process');
      execFileSync(plusdriveBinary(), [
        'build',
        path.join(dir, 'seed-card'),
        '-o',
        cli,
        '--device',
        device,
        '--no-project',
      ]);
      expect(recordsIn(card, device)).toEqual([null, null]);
      const copies = recordsIn(cli, device);
      expect(copies[0]).toEqual(decodeRecord(encodeRecord({ paths: ['modules/a.dkm'] })));
      expect(copies[1]).toEqual(copies[0]);
    },
  );
});

describe('card content id of a linked card', () => {
  const base = (key?: string) => {
    const dir = tmp();
    fs.writeFileSync(path.join(dir, 'card.img'), 'image');
    if (key) fs.writeFileSync(path.join(dir, 'card.key'), key);
    return path.join(dir, 'card.img');
  };
  const keyOf = (card: string) => fs.readFileSync(path.join(path.dirname(card), 'card.key'), 'utf8');

  test('the same base and writes give the same id in another directory', () => {
    const baseCard = base('base-key');
    const a = linkCard(baseCard, path.join(tmp(), 'one'), Buffer.from('writes'));
    const b = linkCard(baseCard, path.join(tmp(), 'two'), Buffer.from('writes'));
    expect(keyOf(a)).toBe(keyOf(b));
  });

  test('other writes or another base give another id, never the id of the base', () => {
    const baseCard = base('base-key');
    const a = linkCard(baseCard, path.join(tmp(), 'one'), Buffer.from('writes'));
    const b = linkCard(baseCard, path.join(tmp(), 'two'), Buffer.from('other writes'));
    const c = linkCard(base('another-key'), path.join(tmp(), 'three'), Buffer.from('writes'));
    expect(new Set([keyOf(a), keyOf(b), keyOf(c), 'base-key']).size).toBe(4);
  });

  test('a base without an id gives a card without one, and an old id in the directory goes', () => {
    const dir = path.join(tmp(), 'card');
    fs.mkdirSync(dir);
    fs.writeFileSync(path.join(dir, 'card.key'), 'stale');
    const card = linkCard(base(), dir, Buffer.from('writes'));
    expect(fs.existsSync(path.join(path.dirname(card), 'card.key'))).toBe(false);
  });
});

describe('system package on the card', () => {
  const ready = fs.existsSync(plusdriveBinary()) && fs.existsSync(systemPackageFile());
  /** The store paths of a card as `plusdrive store ls` lists them, with the package. */
  const storePaths = (card: string) =>
    execFileSync(plusdriveBinary(), ['store', 'ls', card, '--device', device], { encoding: 'utf8' })
      .trim()
      .split('\n')
      .slice(1)
      .map((line) => line.split('\t')[0]);

  test.skipIf(!ready)(
    'a built card holds the package at its store path, with modules or from a folder, and not when left off',
    async () => {
      const dir = tmp();
      const file = path.join(dir, 'a.dkm');
      fs.writeFileSync(file, Buffer.alloc(64));
      const withModule = await buildCard(device, { dir, name: 'm', modules: [{ name: 'modules/a.dkm', file }] });
      expect(storePaths(withModule)).toEqual(expect.arrayContaining(['modules/a.dkm', SYSTEM_PACKAGE_STORE_PATH]));

      const folder = path.join(dir, 'folder');
      fs.mkdirSync(folder);
      fs.writeFileSync(path.join(folder, 'note.txt'), 'x');
      const fromFolder = await buildCard(device, { dir, name: 'f', folder });
      expect(storePaths(fromFolder)).toContain(SYSTEM_PACKAGE_STORE_PATH);
      expect(fs.readdirSync(folder), "the caller's folder is left as it was").toEqual(['note.txt']);

      const without = await buildCard(device, { dir, name: 'n', systemPackage: false });
      expect(() => storePaths(without), 'no file, so no store at all').toThrow('no module store');
    },
  );
});

describe('boot info final', () => {
  const code = (name: string) => LOAD_STATE.find(([, n]) => n === name)![0];
  const info = (loaded: number, ...names: string[]) => ({ loaded, states: names.map(code) });

  test('every module has a final state and loaded counts the active ones', () => {
    expect(bootInfoFinal(info(2, 'ACTIVE', 'ACTIVE'))).toBe(true);
    expect(bootInfoFinal(info(1, 'ACTIVE', 'REFUSED'))).toBe(true);
    expect(bootInfoFinal(info(0, 'SKIPPED', 'SKIPPED'))).toBe(true);
  });

  test('a state already set with loaded not yet raised is not final', () => {
    expect(bootInfoFinal(info(1, 'ACTIVE', 'ACTIVE'))).toBe(false);
  });

  test('a module still loading is not final', () => {
    expect(bootInfoFinal(info(1, 'ACTIVE', 'DSP_LOADING'))).toBe(false);
    expect(bootInfoFinal(info(0, 'SELECTED', 'SELECTED'))).toBe(false);
  });
});

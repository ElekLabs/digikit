import { describe, expect, test } from 'vitest';
import type { Prepared } from '../src/dksample.ts';
import { openSamples } from '../src/samples.ts';
import { FakeDataApi, SLOT } from './data-api-fake.ts';
import { image } from './slot-image.ts';

const FORMAT = {
  path: '/samples',
  count: 64,
  bytesMax: 0x7ffc0,
  rate: 48000,
  channels: 1,
  bits: 16,
  byteOrder: 'big' as const,
};
const converter = {
  upload() {
    throw new Error('unused');
  },
  slot() {
    throw new Error('unused');
  },
};
const open = (fake = new FakeDataApi(), slots = FORMAT) => ({
  fake,
  dev: openSamples('stock-slots', { session: fake as never, slots, converter }),
});
/** A Prepared slot image of `n` samples (n >= 1). */
const prepared = (name: string, n: number, salt = 1): Prepared => {
  const b = image(
    name,
    Array.from({ length: n }, (_, i) => (i * 31 + salt) & 0x7fff),
  );
  return { name, bytes: b, stored: b, kind: 'converted' };
};
const writes = (fake: FakeDataApi) => fake.sent.filter((r) => r.type >= 0x57 && r.type <= 0x59);
const zero = (b: Uint8Array) => b.every((x) => x === 0);

describe('the slot backend on a fresh device', () => {
  test('lists nothing, has no folders and no rename, and is no bulk backend', async () => {
    const { dev } = open();
    expect(await dev.list('')).toEqual([]);
    expect(await dev.space()).toEqual({ free: 64 * 0x7ffc0, total: 64 * 0x7ffc0 });
    expect(await dev.canMkdir()).toBe(false);
    expect(await dev.canRename()).toBe(false);
    await expect(dev.mkdir('x')).rejects.toMatchObject({ kind: 'usage' });
    await expect(dev.rmdir('x')).rejects.toMatchObject({ kind: 'usage' });
    await expect(dev.removeTree('x')).rejects.toMatchObject({ kind: 'usage' });
    await expect(dev.rename('', 'a', '', 'b')).rejects.toMatchObject({ kind: 'usage' });
    const withBulk = openSamples('stock-slots', { session: {} as never, slots: FORMAT, converter, bulk: {} as never });
    expect(withBulk.bulkState).toBeUndefined();
  });
});

describe('put', () => {
  test('lands in slot 1, is listed, reports progress up to the image size, and verifies', async () => {
    const { fake, dev } = open();
    const p = prepared('kick', 2);
    const seen: [number, number][] = [];
    await dev.put('', p, (sent, total) => seen.push([sent, total]));
    expect(await dev.list('')).toEqual([{ name: 'kick', kind: 'file', size: 4, stored: 0x44, slot: 1 }]);
    expect(Buffer.from(fake.slotBytes(1).subarray(0, p.bytes.length)).equals(Buffer.from(p.bytes))).toBe(true);
    for (let s = 2; s <= 70; s++) expect(zero(fake.slotBytes(s))).toBe(true);
    expect(seen.at(-1)).toEqual([p.bytes.length, p.bytes.length]);
    expect(seen.every(([sent, total]) => sent <= total && total === p.bytes.length)).toBe(true);
    const v = await dev.verify('', p);
    expect(v.ok).toBe(true);
    expect(v.mismatchAt).toBeNull();
  });

  test('uses the lowest free slot, the named slot when given, and never overwrites without a name', async () => {
    const { fake, dev } = open();
    await dev.put('', prepared('a', 3));
    await dev.put('', prepared('b', 3));
    await dev.put('', prepared('c', 3), undefined, { slot: 5 });
    await dev.put('', prepared('d', 3), undefined, { slot: 1 });
    expect([1, 2, 3, 4, 5].map((s) => fake.used(s))).toEqual([true, true, false, false, true]);
    expect(fake.nameOf(1)).toBe('d');
    await dev.put('', prepared('e', 3, 1));
    expect(fake.nameOf(3)).toBe('e');
    await dev.put('', prepared('e', 3, 2));
    expect(fake.nameOf(4)).toBe('e');
    expect((await dev.list('')).map((e) => e.name)).toEqual(['d', 'b', 'e #3', 'e #4', 'c']);
  });

  test('a full device refuses a put without a slot and sends no write', async () => {
    const { fake, dev } = open();
    for (let s = 1; s <= 64; s++) fake.load(s, image(`s${s}`, [1]));
    await expect(dev.put('', prepared('x', 2))).rejects.toMatchObject({ status: 'E_FULL' });
    expect(writes(fake).length).toBe(0);
  });

  test('refuses before the first request: too long, bad slot, bad image, a folder, a slot past the count', async () => {
    const { fake, dev } = open();
    const big = image(
      'big',
      Array.from({ length: 0x3ffe1 }, () => 1),
    );
    await expect(dev.put('', { name: 'big', bytes: big, stored: big, kind: 'converted' })).rejects.toThrow(
      /at most 524224/,
    );
    for (const slot of [0, 65, 2.5]) await expect(dev.put('', prepared('a', 2), undefined, { slot })).rejects.toThrow();
    const bad = Uint8Array.from(prepared('a', 2).bytes);
    bad[11] = 9; // the length word says 9 samples
    await expect(dev.put('', { name: 'a', bytes: bad, stored: bad, kind: 'converted' })).rejects.toThrow(/length word/);
    await expect(dev.put('x', prepared('a', 2))).rejects.toMatchObject({ kind: 'usage' });
    expect(fake.sent.length).toBe(0);
    const eight = open(new FakeDataApi(), { ...FORMAT, count: 8 });
    await expect(eight.dev.put('', prepared('a', 2), undefined, { slot: 9 })).rejects.toMatchObject({ kind: 'usage' });
    expect(eight.fake.sent.length).toBe(0);
  });

  test('the largest sample fits slot 64 and writes nothing past it', async () => {
    const { fake, dev } = open();
    const img = image(
      'm',
      Array.from({ length: 0x3ffe0 }, () => 1),
    );
    expect(img.length).toBe(SLOT);
    await dev.put('', { name: 'm', bytes: img, stored: img, kind: 'converted' }, undefined, { slot: 64 });
    const wire = fake.sent
      .filter((r) => r.type === 0x58)
      .reduce((n, r) => n + Buffer.from(r.payload).readUInt32BE(12), 0);
    expect(wire).toBe(31 + SLOT + 12);
    expect(zero(fake.card.subarray(64 * SLOT))).toBe(true);
    expect(zero(fake.slotBytes(63))).toBe(true);
  });

  test('a cancel in the middle leaves the slot empty, and a new put works', async () => {
    const { fake, dev } = open();
    const p = prepared('long', 5000);
    const ctl = new AbortController();
    let calls = 0;
    await expect(
      dev.put(
        '',
        p,
        () => {
          if (++calls === 2) ctl.abort();
        },
        { signal: ctl.signal },
      ),
    ).rejects.toMatchObject({ kind: 'cancelled' });
    expect(await dev.list('')).toEqual([]);
    await dev.put('', p);
    expect(fake.used(1)).toBe(true);
  });

  test('a lost link is the error of the put and leaves the slot empty', async () => {
    const { fake, dev } = open();
    fake.dropAfter = 3;
    await expect(dev.put('', prepared('long', 5000))).rejects.toThrow(/link lost/);
    fake.dropAfter = undefined;
    expect(fake.used(1)).toBe(false);
  });
});

describe('remove and the names of the listing', () => {
  test('a duplicate name is listed with its slot and removed by that name only', async () => {
    const { fake, dev } = open();
    await dev.put('', prepared('a', 3, 1));
    await dev.put('', prepared('b', 3));
    await dev.put('', prepared('a', 3, 2));
    expect((await dev.list('')).map((e) => e.name)).toEqual(['a #1', 'b', 'a #3']);
    const one = Uint8Array.from(fake.slotBytes(1));
    const two = Uint8Array.from(fake.slotBytes(2));
    await dev.remove('', 'a #3');
    expect(zero(fake.slotBytes(3).subarray(0, 0x200))).toBe(true);
    expect(fake.used(3)).toBe(false);
    expect(Buffer.from(fake.slotBytes(1)).equals(Buffer.from(one))).toBe(true);
    expect(Buffer.from(fake.slotBytes(2)).equals(Buffer.from(two))).toBe(true);
    const clears = fake.sent.filter((r) => r.type === 0x5c).length;
    await expect(dev.remove('', 'nope')).rejects.toMatchObject({ status: 'E_NOFILE' });
    expect(fake.sent.filter((r) => r.type === 0x5c).length).toBe(clears);
    expect(fake.foreign).toEqual([]);
  });

  test('same on device is never true: the listing has no hash, so every file is sent', async () => {
    const { dev } = open();
    await dev.put('', prepared('x', 2));
    const listing = await dev.list('');
    expect(dev.sameOnDevice(prepared('x', 2, 9), listing)).toBe(false);
    expect(dev.sameOnDevice(prepared('x', 2), listing)).toBe(false);
  });

  test('a name that more than one slot is listed as is refused and clears nothing', async () => {
    const { fake, dev } = open();
    fake.load(1, image('a #3', [1, 2]));
    fake.load(3, image('a', [3, 4]));
    fake.load(4, image('a', [5, 6]));
    expect((await dev.list('')).map((e) => e.name)).toEqual(['a #3', 'a #3', 'a #4']);
    await expect(dev.remove('', 'a #3')).rejects.toThrow(/ambiguous name: 2 slots/);
    expect([1, 3, 4].map((s) => fake.used(s))).toEqual([true, true, true]);
    expect(fake.sent.filter((r) => r.type === 0x5c).length).toBe(0);
  });
});

describe('verify', () => {
  test('finds the first byte that differs, and says so for a file the device does not have', async () => {
    const { fake, dev } = open();
    const p = prepared('v', 20);
    await dev.put('', p);
    fake.card[fake.off(1) + 0x43] ^= 1;
    const v = await dev.verify('', p);
    expect(v.ok).toBe(false);
    expect(v.mismatchAt).toBe(0x43);
    const none = await dev.verify('', prepared('zz', 2));
    expect(none.ok).toBe(false);
    expect(none.device).toBeNull();
  });
});

describe('opening the backend', () => {
  test('refuses a missing layout, another route, too many slots and a sample that does not fit a slot', () => {
    const session = {} as never;
    expect(() => openSamples('stock-slots', { session })).toThrow(/slot layout/);
    expect(() => openSamples('stock-slots', { session, slots: { ...FORMAT, path: '/projects' } })).toThrow(
      /\/samples only/,
    );
    expect(() => openSamples('stock-slots', { session, slots: { ...FORMAT, count: 65 } })).toThrow();
    expect(() => openSamples('stock-slots', { session, slots: { ...FORMAT, bytesMax: 0x7ffc2 } })).toThrow();
  });
  test('prepare calls the slot converter with the layout; the size is the image size', () => {
    const calls: unknown[][] = [];
    const made = prepared('p', 2);
    const rec = {
      upload: converter.upload,
      slot: (...a: unknown[]) => {
        calls.push(a);
        return made;
      },
    };
    const dev = openSamples('stock-slots', { session: {} as never, slots: FORMAT, converter: rec });
    const raw = Uint8Array.of(1, 2);
    expect(dev.prepare('x.wav', raw)).toBe(made);
    expect(calls).toEqual([['x.wav', raw, FORMAT]]);
    expect(dev.sizeOf(made)).toBe(made.bytes.length);
  });
});

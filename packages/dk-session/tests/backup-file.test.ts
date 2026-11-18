import { describe, expect, test } from 'vitest';
import { SLOT_IMAGE_MAX, fwCrc, unwrapBackup, wrapBackup } from '../src/backup-file.ts';
import { image } from './slot-image.ts';

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex');

describe('the firmware CRC', () => {
  test('is the CRC-32 with seed 0xffffffff: the check value, and chaining', () => {
    const t = new TextEncoder().encode('123456789');
    expect(fwCrc(t)).toBe(0xd202d277);
    expect(fwCrc(t.subarray(4), fwCrc(t.subarray(0, 4)))).toBe(0xd202d277);
    expect(fwCrc(Uint8Array.of(1, 2, 3))).toBe(0x5502a6f0);
  });
});

describe('the backup file', () => {
  test('wraps a slot image: 31-byte header, the image, the 12-byte footer (layout of finding 142)', () => {
    const img = image('ab', [1, 0x7fff]);
    const f = wrapBackup(3, img);
    expect(f.length).toBe(31 + 0x44 + 12);
    expect(hex(f.subarray(0, 31))).toBe('ac11d303020008000d0000000000000008ffffffff0000000300000044000c');
    expect(hex(f.subarray(31, 31 + 0x44))).toBe(hex(img));
    expect(hex(f.subarray(31 + 0x44))).toBe('68d0a8cc00000000aaa1daaa');
  });
  test('unwraps what it wrapped, and refuses a bad CRC, a wrong type, a compressed file and a truncation', () => {
    const img = image('kick', [5, -5, 9]);
    const f = wrapBackup(64, img);
    expect(unwrapBackup(f)).toEqual({ slot: 64, image: img });
    const flip = (at: number, v: number) => Uint8Array.from(f, (x, i) => (i === at ? v : x));
    expect(() => unwrapBackup(flip(31 + 0x41, 0x55))).toThrow(/CRC/);
    expect(() => unwrapBackup(flip(0x0d + 3, 7))).toThrow(/not a sample/);
    expect(() => unwrapBackup(flip(0x1d, 1))).toThrow(/compressed/);
    expect(() => unwrapBackup(f.subarray(0, f.length - 1))).toThrow(/truncated/);
  });
  test('refuses to wrap what the firmware would write past its slot or read as garbage', () => {
    const img = image('a', [1]);
    for (const slot of [0, 65, 1.5, -1]) expect(() => wrapBackup(slot, img)).toThrow(/slots are 1 to 64/);
    expect(() => wrapBackup(1, new Uint8Array(SLOT_IMAGE_MAX + 2))).toThrow(/at most 524288/);
    const bad = Uint8Array.from(img);
    bad[8 + 3] = 9; // the length word says 9 samples, the image holds 1
    expect(() => wrapBackup(1, bad)).toThrow(/length word/);
    expect(() =>
      wrapBackup(
        1,
        Uint8Array.from(img, (x, i) => (i === 0 ? 0x58 : x)),
      ),
    ).toThrow(/SAMP/);
  });
  test('accepts the largest image: 262,112 samples, 0x80000 bytes', () => {
    const max = image(
      'm',
      Array.from({ length: 0x3ffe0 }, () => 1),
    );
    expect(max.length).toBe(SLOT_IMAGE_MAX);
    expect(wrapBackup(1, max).length).toBe(31 + SLOT_IMAGE_MAX + 12);
  });
});

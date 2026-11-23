import { inflateSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { oledPng } from '../src/oled-png.ts';

/** The decoded grayscale rows of a PNG made by oledPng (one filter byte per row). */
function decode(png: Buffer): { w: number; h: number; rows: Buffer[] } {
  expect(png.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
  const w = png.readUInt32BE(16);
  const h = png.readUInt32BE(20);
  const len = png.readUInt32BE(33);
  expect(png.subarray(37, 41).toString()).toBe('IDAT');
  const raw = inflateSync(png.subarray(41, 41 + len));
  const rows = Array.from({ length: h }, (_, y) => raw.subarray(y * (w + 1) + 1, (y + 1) * (w + 1)));
  return { w, h, rows };
}

describe('oledPng', () => {
  it('lights pixel (x, y) from bit y%8 of byte (7 - y/8) + 8*x, scaled', () => {
    const frame = new Uint8Array(1024);
    frame[7 - 1 + 8 * 5] = 1 << 3; // x 5, y 11
    frame[7] = 1; // x 0, y 0
    const { w, h, rows } = decode(oledPng(frame, 2));
    expect([w, h]).toEqual([256, 128]);
    const lit = new Set<string>();
    rows.forEach((row, y) => row.forEach((v, x) => v === 255 && lit.add(`${x >> 1},${y >> 1}`)));
    expect([...lit].sort()).toEqual(['0,0', '5,11']);
    expect(rows[22]![10]).toBe(255);
    expect(rows[23]![11]).toBe(255);
    expect(rows[24]![10]).toBe(0);
  });

  it('refuses a frame of the wrong size', () => {
    expect(() => oledPng(new Uint8Array(10))).toThrow('1024');
  });
});

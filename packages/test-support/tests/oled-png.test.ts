import fs from 'node:fs';
import { inflateSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { oledPng } from '../src/oled-png.ts';

// The input of the golden: bit 0, bit 6 and bit 7 of scattered bytes, so a wrong bit or byte order moves pixels.
const frame = Uint8Array.from(
  { length: 1024 },
  (_, i) => (i % 13 === 0 ? 0x01 : 0) | (i % 29 === 0 ? 0x40 : 0) | (i % 97 === 0 ? 0x80 : 0),
);

/** Size and inflated scanlines of a PNG (the compressed bytes depend on the zlib build, the pixels do not). */
function decoded(png: Buffer): { header: string; scanlines: string } {
  const length = png.readUInt32BE(33);
  return {
    header: png.subarray(16, 29).toString('hex'),
    scanlines: inflateSync(png.subarray(41, 41 + length)).toString('hex'),
  };
}

describe('oledPng', () => {
  it('writes the PNG of the frame bytes the Python oled_png of tools/control/digi_client.py wrote (golden, scale 2)', () => {
    const golden = fs.readFileSync(new URL('./fixtures/oled-frame-golden.png', import.meta.url));
    expect(decoded(oledPng(frame, 2))).toEqual(decoded(golden));
  });

  it('refuses a frame of the wrong size', () => {
    expect(() => oledPng(new Uint8Array(10))).toThrow('1024');
  });
});

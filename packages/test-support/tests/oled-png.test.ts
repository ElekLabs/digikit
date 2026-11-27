import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { inflateSync } from 'node:zlib';
import { afterEach, describe, expect, it } from 'vitest';
import { oledPng, renderShots } from '../src/oled-png.ts';

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

describe('renderShots', () => {
  const dirs: string[] = [];
  const tmp = () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-shots-'));
    dirs.push(dir);
    return dir;
  };
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it('writes a PNG of every .oled frame next to it at 3 times the size, in the order of the names, and copies them to the shots directory', () => {
    const dir = tmp();
    const copy = path.join(tmp(), 'shots');
    fs.writeFileSync(path.join(dir, 'b.oled'), frame);
    fs.writeFileSync(
      path.join(dir, 'a.oled'),
      frame.map((v) => v ^ 0xff),
    );
    fs.writeFileSync(path.join(dir, 'note.txt'), 'not a frame');
    const result = renderShots(dir, copy);
    expect(result).toEqual({ files: [path.join(dir, 'a.png'), path.join(dir, 'b.png')], count: 2 });
    expect(fs.readFileSync(path.join(dir, 'b.png')).readUInt32BE(16)).toBe(128 * 3);
    expect(decoded(fs.readFileSync(path.join(dir, 'b.png')))).toEqual(decoded(oledPng(frame, 3)));
    expect(fs.readdirSync(copy).sort()).toEqual(['a.png', 'b.png']);
  });

  it('refuses a missing directory and a frame of the wrong size, and writes nothing then', () => {
    expect(() => renderShots(path.join(tmp(), 'absent'), undefined)).toThrow('not a shots directory');
    const dir = tmp();
    fs.writeFileSync(path.join(dir, 'a.oled'), frame);
    fs.writeFileSync(path.join(dir, 'b.oled'), Buffer.alloc(10));
    expect(() => renderShots(dir, undefined)).toThrow('expected 1024 OLED bytes, got 10');
    expect(fs.readdirSync(dir).filter((name) => name.endsWith('.png'))).toEqual([]);
  });
});

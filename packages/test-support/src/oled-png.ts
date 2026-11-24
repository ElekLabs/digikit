import fs from 'node:fs';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';

import { OLED_FRAME_BYTES, OLED_HEIGHT, OLED_WIDTH, oledPixel } from '@digi/workbench-core/oled-frame.ts';

/** A grayscale PNG of the OLED from its 1024 decoded bytes (layout: workbench-core `oled-frame.ts`), each pixel `scale` times. */
export function oledPng(frame: Uint8Array, scale = 4): Buffer {
  if (frame.length !== OLED_FRAME_BYTES) throw new Error(`OLED frame is ${frame.length} bytes, expected 1024`);
  const w = OLED_WIDTH * scale;
  const h = OLED_HEIGHT * scale;
  const raw = Buffer.alloc((w + 1) * h);
  for (let y = 0; y < h; y++) {
    const sy = Math.floor(y / scale);
    const row = y * (w + 1);
    for (let x = 0; x < w; x++) {
      const sx = Math.floor(x / scale);
      raw[row + 1 + x] = oledPixel(frame, sx, sy) ? 255 : 0;
    }
  }
  const chunk = (type: string, body: Buffer): Buffer => {
    const head = Buffer.alloc(4);
    head.writeUInt32BE(body.length);
    const tail = Buffer.alloc(4);
    tail.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type), body])));
    return Buffer.concat([head, Buffer.from(type), body, tail]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 0, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** What `renderShots` wrote: the PNG files and their count. */
export interface RenderedShots {
  files: string[];
  count: number;
}

/**
 * Render the raw OLED frames (`*.oled`, 1024 bytes each) saved in `directory` as PNG files next to them (3 times the size), in the order of their names. With `copyTo` (the
 * `DIGIKIT_SHOTS` directory of a run) each PNG is copied there too. A frame of another size, or a missing directory, is an error and nothing is written.
 */
export function renderShots(directory: string, copyTo: string | undefined = process.env.DIGIKIT_SHOTS): RenderedShots {
  if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory())
    throw new Error(`not a shots directory: ${directory}`);
  const frames = fs
    .readdirSync(directory)
    .filter((name) => name.endsWith('.oled'))
    .sort()
    .map((name) => ({ name, raw: fs.readFileSync(path.join(directory, name)) }));
  for (const { name, raw } of frames)
    if (raw.length !== OLED_FRAME_BYTES)
      throw new Error(`${path.join(directory, name)}: expected ${OLED_FRAME_BYTES} OLED bytes, got ${raw.length}`);
  if (copyTo) fs.mkdirSync(copyTo, { recursive: true });
  const files = frames.map(({ name, raw }) => {
    const destination = path.join(directory, `${name.slice(0, -'.oled'.length)}.png`);
    fs.writeFileSync(destination, oledPng(raw, 3));
    if (copyTo) {
      const copied = path.join(copyTo, path.basename(destination));
      if (path.resolve(copied) !== path.resolve(destination)) fs.copyFileSync(destination, copied);
    }
    return destination;
  });
  return { files, count: files.length };
}

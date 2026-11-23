import { crc32, deflateSync } from 'node:zlib';

export const OLED_WIDTH = 128;
export const OLED_HEIGHT = 64;

/** A grayscale PNG of the 128x64 OLED from its 1024 decoded bytes (pixel (x, y) is bit y%8 of byte (7 - y/8) + 8*x), each pixel `scale` times. */
export function oledPng(frame: Uint8Array, scale = 4): Buffer {
  if (frame.length !== (OLED_WIDTH * OLED_HEIGHT) / 8)
    throw new Error(`OLED frame is ${frame.length} bytes, expected 1024`);
  const w = OLED_WIDTH * scale;
  const h = OLED_HEIGHT * scale;
  const raw = Buffer.alloc((w + 1) * h);
  for (let y = 0; y < h; y++) {
    const sy = Math.floor(y / scale);
    const row = y * (w + 1);
    for (let x = 0; x < w; x++) {
      const sx = Math.floor(x / scale);
      raw[row + 1 + x] = (frame[7 - (sy >> 3) + 8 * sx]! >> (sy & 7)) & 1 ? 255 : 0;
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

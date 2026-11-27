import { describe, expect, it } from 'vitest';
import { OLED_HEIGHT, OLED_WIDTH, oledPixel } from '@digi/workbench-core/oled-frame.ts';
import { screenBytes } from '../preview/frame-from-png';

describe('screenBytes', () => {
  it('reads back the frame bytes of an image drawn from them at scale 3', () => {
    const frame = Uint8Array.from(
      { length: 1024 },
      (_, i) => (i % 13 === 0 ? 0x01 : 0) | (i % 29 === 0 ? 0x40 : 0) | (i % 97 === 0 ? 0x80 : 0),
    );
    const scale = 3;
    const width = OLED_WIDTH * scale;
    const height = OLED_HEIGHT * scale;
    const data = new Uint8ClampedArray(width * height * 4);
    for (let y = 0; y < height; y++)
      for (let x = 0; x < width; x++)
        if (oledPixel(frame, Math.floor(x / scale), Math.floor(y / scale)))
          data.fill(255, (y * width + x) * 4, (y * width + x) * 4 + 3);
    expect(screenBytes({ data, width, height } as ImageData)).toEqual(frame);
  });
});

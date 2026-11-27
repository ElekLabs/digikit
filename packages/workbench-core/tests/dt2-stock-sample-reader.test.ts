import { describe, expect, it, vi } from 'vitest';
import { dt2StockSampleReader, matchesNativeSample } from '../src/reference/dt2-stock-sample-reader.ts';

const hash = 'b9694553';
const size = 19_280;
const firmware = '3'.repeat(64);
const port = `
[port]
device = "dt2"
version = "1.17"
syx_sha256 = "${firmware}"
[[anchor]]
name = "cf.samples.site.slot.keys"
cpu = "cf"
addr = 0x40150300
len = 4
`;

function bytes(...words: number[]): Uint8Array {
  const view = new DataView(new ArrayBuffer(4 * words.length + 1), 1);
  words.forEach((word, i) => view.setUint32(i * 4, word));
  return new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
}

describe('DT2 stock sample reader', () => {
  it('follows the resolved address and reads the identity of the occupied slot', async () => {
    const peek = vi
      .fn()
      .mockResolvedValueOnce(bytes(0x405b1368))
      .mockResolvedValueOnce(bytes(3, 0xb9694553, size, 8));
    const key = await dt2StockSampleReader(port, firmware).readLoadedKey({ peek }, 129);
    expect(peek.mock.calls).toEqual([
      [0x40150300, 4],
      [0x405b1b78, 16],
    ]);
    expect(matchesNativeSample(key, { content_hash: hash, stored_bytes: size })).toBe(true);
    expect(matchesNativeSample(key, { content_hash: hash, stored_bytes: size - 16 })).toBe(false);
    expect(matchesNativeSample(key, { content_hash: 'f9694553', stored_bytes: size })).toBe(false);
  });

  it('treats reserved and released slots as unloaded and refuses out-of-range slots', async () => {
    const peek = vi
      .fn()
      .mockResolvedValueOnce(bytes(0x405b1368))
      .mockResolvedValueOnce(bytes(0xffffffff, 0, 0, 0));
    const reader = dt2StockSampleReader(port, firmware);
    await expect(reader.readLoadedKey({ peek }, 128)).resolves.toBeNull();
    await expect(reader.readLoadedKey({ peek }, 1024)).rejects.toThrow('Invalid stock sample slot');
    expect(peek).not.toHaveBeenCalled();
    await expect(reader.readLoadedKey({ peek }, 1)).resolves.toBeNull();
    expect(matchesNativeSample(null, { content_hash: hash, stored_bytes: size })).toBe(false);
  });

  it('refuses a different firmware or missing binding before reading memory', () => {
    expect(() => dt2StockSampleReader(port, '4'.repeat(64))).toThrow('does not match');
    expect(() => dt2StockSampleReader(port.replace('len = 4', 'len = 2'), firmware)).toThrow('No stock sample key');
  });
});

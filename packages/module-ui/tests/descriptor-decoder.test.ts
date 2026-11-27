import { describe, expect, it } from 'vitest';
import { decodeDescriptor, encodeDescriptor, InvalidDescriptor } from '../src/index.ts';
import { bytesOf, declaredSchema, vectors } from './vectors.ts';

describe('decodeDescriptor against the shared vectors', () => {
  for (const c of vectors.descriptors) {
    it(`${c.name}: the bytes decode to the schema its TOML declares`, () => {
      expect(decodeDescriptor(bytesOf(c.bytes_hex))).toEqual(declaredSchema(c));
    });
    it(`${c.name}: every truncation is refused`, () => {
      const bytes = bytesOf(c.bytes_hex);
      for (let n = 0; n < bytes.length; n++)
        expect(() => decodeDescriptor(bytes.subarray(0, n))).toThrow(InvalidDescriptor);
    });
    it(`${c.name}: a trailing byte is refused`, () => {
      expect(() => decodeDescriptor(Uint8Array.of(...bytesOf(c.bytes_hex), 0))).toThrow(/Unexpected/);
    });
  }
});

describe('encodeDescriptor against the shared vectors', () => {
  for (const c of vectors.descriptors) {
    it(`${c.name}: the declared schema encodes to patchkit's bytes`, () => {
      const schema = declaredSchema(c);
      expect(encodeDescriptor(schema.name, schema)).toEqual(bytesOf(c.bytes_hex));
    });
  }
});

describe('decodeDescriptor refusals', () => {
  const typed = bytesOf(vectors.descriptors.find((c) => c.name === 'v2_all_kinds')!.bytes_hex);
  const v1 = bytesOf(vectors.descriptors.find((c) => c.name === 'v1_text_and_file_name')!.bytes_hex);
  // [version][len+name][count]  then a setting `[len+id][len+label][stored][kind][apply]`
  const one = (kind: number, stored: number, ...rest: number[]) =>
    Uint8Array.of(1, 1, 0x4d, 1, 1, 0x61, 1, 0x41, stored, kind, 0, ...rest);

  it('refuses a version 1 descriptor that holds a new kind', () => {
    expect(() => decodeDescriptor(Uint8Array.of(1, ...typed.subarray(1)))).toThrow(/version 1/);
  });
  it('refuses an unknown kind and an unknown version', () => {
    expect(() => decodeDescriptor(Uint8Array.of(2, 1, 0x4d, 1, 1, 0x61, 1, 0x41, 1, 9, 0))).toThrow(/Unsupported/);
    expect(() => decodeDescriptor(Uint8Array.of(3, ...v1.subarray(1)))).toThrow(/Unsupported/);
  });
  it('refuses more than 512 bytes and a duplicate id', () => {
    expect(() => decodeDescriptor(new Uint8Array(513).fill(1))).toThrow(/Unsupported/);
    const text = [1, 0x61, 1, 0x41, 8, 0, 0];
    expect(() => decodeDescriptor(Uint8Array.of(1, 1, 0x4d, 2, ...text, ...text))).toThrow(/Invalid public setting/);
  });
  it('refuses a text size of 0 or above 64 and a mismatch between stored size and kind', () => {
    expect(() => decodeDescriptor(one(0, 0))).toThrow(/text setting/);
    expect(() => decodeDescriptor(one(0, 65))).toThrow(/text setting/);
    expect(decodeDescriptor(one(0, 64)).settings).toHaveLength(1);
    // boolean with stored size 2
    expect(() => decodeDescriptor(Uint8Array.of(2, 1, 0x4d, 1, 1, 0x61, 1, 0x41, 2, 3, 0, 0))).toThrow(/boolean/);
  });
  it('refuses an integer whose max is not above min, whose step does not divide the span, or whose default is off the step', () => {
    const int = (min: number, max: number, step: number, def: number, decimals = 0) =>
      Uint8Array.of(
        2,
        1,
        0x4d,
        1,
        1,
        0x61,
        1,
        0x41,
        4,
        2,
        0,
        ...be(min),
        ...be(max),
        ...be(step),
        decimals,
        ...be(def),
        0,
      );
    expect(decodeDescriptor(int(0, 10, 2, 4)).settings).toHaveLength(1);
    expect(() => decodeDescriptor(int(5, 5, 1, 5))).toThrow(/integer setting/);
    expect(() => decodeDescriptor(int(0, 10, 3, 0))).toThrow(/integer setting/);
    expect(() => decodeDescriptor(int(0, 10, 2, 3))).toThrow(/integer default/);
    expect(() => decodeDescriptor(int(0, 10, 0, 0))).toThrow(/integer setting/);
    expect(() => decodeDescriptor(int(0, 10, 1, 0, 5))).toThrow(/integer setting/);
  });
  it('refuses a choice default past the count, a duplicate label and 33 labels, and accepts 32', () => {
    const choice = (labels: string[], def: number) =>
      Uint8Array.of(
        2,
        1,
        0x4d,
        1,
        1,
        0x61,
        1,
        0x41,
        1,
        4,
        0,
        labels.length,
        def,
        ...labels.flatMap((l) => [l.length, ...new TextEncoder().encode(l)]),
      );
    const names = (n: number) => Array.from({ length: n }, (_, i) => `c${i}`);
    expect(decodeDescriptor(choice(names(32), 31)).settings[0]).toMatchObject({ kind: 'choice', default: 'c31' });
    expect(() => decodeDescriptor(choice(names(33), 0))).toThrow(/choice setting/);
    expect(() => decodeDescriptor(choice(names(2), 2))).toThrow(/choice setting/);
    expect(() => decodeDescriptor(choice(['a', 'a'], 0))).toThrow(/choice setting/);
  });
});

function be(v: number) {
  return [v >>> 24, v >>> 16, v >>> 8, v].map((b) => b & 255);
}

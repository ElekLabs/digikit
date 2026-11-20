/** A slot image: SAMP, +4 zero, the length in samples at +8, the name at +0xC, the samples at +0x40. */
export function image(name: string, samples: number[]): Uint8Array {
  const b = Buffer.alloc(0x40 + samples.length * 2);
  b.write('SAMP', 0);
  b.writeUInt32BE(samples.length, 8);
  b.write(name, 0xc);
  samples.forEach((s, i) => b.writeInt16BE(s, 0x40 + i * 2));
  return new Uint8Array(b); // a plain Uint8Array, as the code under test returns
}

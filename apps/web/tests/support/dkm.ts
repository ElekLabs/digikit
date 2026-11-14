// Fake modules and the needs stand-in of the Transfer logic tests (dktransfer.test.ts, dktransfer.wasm.test.ts).
import type { FitReader } from '@digi/workbench-core/modules/fit.ts';
import type { Needs } from '@digi/workbench-core/modules/fit.ts';

export interface DkmOptions {
  machine?: number;
  fx?: number;
  size?: number;
  dsp?: number;
  version?: number;
  salt?: number;
}

/** A .dkm with a valid header (docs/design/digikit-dkm.md) of `size` bytes. */
export function dkm(
  name: string,
  { machine = 0, fx = 0, size = 700, dsp = 0, version = 0x000102, salt = 0 }: DkmOptions = {},
): Uint8Array {
  const b = new Uint8Array(size);
  b.set([0x44, 0x4b, 0x4d, 0x46], 0);
  new DataView(b.buffer).setUint32(8, size);
  new DataView(b.buffer).setUint32(0x34, version);
  b.set(Buffer.from(name), 0x40);
  new DataView(b.buffer).setUint32(0x64, 100);
  new DataView(b.buffer).setUint32(0x68, 20);
  new DataView(b.buffer).setUint32(0x9c, dsp);
  for (let i = 0xa0; i < size; i++) b[i] = (i * 31 + salt) & 255;
  // the keys table (header 0x90/0x94): u16 words at +6 and +8 count machines and effects
  new DataView(b.buffer).setUint32(0x90, 0xa0);
  new DataView(b.buffer).setUint32(0x94, 12);
  b.fill(0, 0xa0, 0xac);
  new DataView(b.buffer).setUint16(0xa0 + 6, machine);
  new DataView(b.buffer).setUint16(0xa0 + 8, fx);
  return b;
}

/**
 * The needs reader of these tests. The needs are inferred by Rust (packages/dkneeds, through patchkit.wasm: dkneeds.wasm.test.ts runs the real one on
 * real files); a fake module here has only a header, so a stand-in gives the fit logic its input: the arena block of image 100 and bss 20, and the
 * DSP area the fake's DSP size word holds.
 */
export const needsOf = (b: Uint8Array): Needs & { file_size: number } => {
  const dsp = new DataView(b.buffer, b.byteOffset, b.length).getUint32(0x9c);
  return { file_size: b.length, ARENA: 120, DSP_AREA: dsp, DSP_ALIGN: dsp ? 8 : 0 };
};

/**
 * A fit rule that finds every module fits, for the host tests that need a reader but do not judge fit (the rule is Rust, run for real by the
 * `*.wasm.test.ts` files).
 */
export const everythingFits: FitReader = {
  fit: ({ needs }) => ({ each: needs.map(() => null), set: null, line: '' }),
};

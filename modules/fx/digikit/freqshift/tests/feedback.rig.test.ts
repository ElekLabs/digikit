import { FxRig, P0, f2w, fxSpec } from '@digi/sharc-rig-kit';
import { expect, test } from 'vitest';

const module = fxSpec('digikit/freqshift')!;
const blockSize = 32;
const delayFrames = 2640;
const feedbackWord = P0 + 4;
const zero = Array<number>(2 * blockSize).fill(0);
const f32 = Math.fround;
const lowpass = f32(-Math.expm1((-2 * Math.PI * 6200) / 48000));
const dc = f32(-Math.expm1((-2 * Math.PI * 65) / 48000));

function open(device: 'dt2' | 'dn2', gain: number, recordStores = false) {
  const shift28Hz = 5.5 * Math.log2(1 + 28 / 0.63);
  return FxRig.open(
    [{ pc: module.process, state: module.state, desc: module.desc, params: [shift28Hz, 4, 0, 1 / 16, gain].map(f2w) }],
    { sources: module.sources, device, recordStores },
  );
}

function burst(block: number, amplitude = 0.3) {
  return zero.map((_, i) => {
    const n = block * blockSize + (i % blockSize);
    return n < 960 && i < blockSize ? amplitude * Math.sin((2 * Math.PI * 440 * n) / 48000) : 0;
  });
}

for (const device of ['dt2', 'dn2'] as const) {
  test(`${device}: feedback matches the filtered 55 ms stereo loop, including the half-block wrap`, async () => {
    const gain = f32(0.48);
    const effect = await open(device, gain, true);
    const shifter = await open(device, 0);
    const dry = await open(device, 0);
    try {
      const ring = new Float32Array(delayFrames * 2);
      const low = [0, 0];
      const offset = [0, 0];
      let error = 0;
      let firstChange = -1;
      for (let k = 0; k < 190; k++) {
        const input = burst(k);
        const index = (i: number) => ((k * blockSize + (i % blockSize)) % delayFrames) * 2 + (i >> 5);
        const loop = input.map((x, i) => f32(f32(x) + f32(gain * ring[index(i)]!)));
        const want = await shifter.block(loop, 'friendly');
        const off = await dry.block(input, 'friendly');
        const got = await effect.block(input);
        if (!got.returned || got.faults[0] || got.ctxBad.length || got.regsBad.length)
          throw new Error(`block ${k}: ${got.halt}, faults ${got.faults}, registers ${got.ctxBad} ${got.regsBad}`);
        expect(effect.foreignStores(0, got.stores)).toEqual([]);
        for (let j = 0; j < 2 * blockSize; j++) {
          const c = j >> 5;
          const sample = want.scratch[j]!;
          low[c] = f32(low[c]! + f32(f32(sample - low[c]!) * lowpass));
          offset[c] = f32(offset[c]! + f32(f32(low[c]! - offset[c]!) * dc));
          ring[index(j)] = f32(low[c]! - offset[c]!);
          error = Math.max(error, Math.abs(got.scratch[j]! - sample));
          if (c === 0 && firstChange < 0 && Math.abs(got.scratch[j]! - off.scratch[j]!) > 1e-6)
            firstChange = k * blockSize + j;
          if (c === 1) expect(got.scratch[j] === 0).toBe(true);
        }
      }
      expect(firstChange).toBeGreaterThanOrEqual(delayFrames);
      expect(firstChange).toBeLessThan(delayFrames + blockSize / 2);
      expect(error).toBeLessThan(2e-6);
    } finally {
      await Promise.all([effect.close(), shifter.close(), dry.close()]);
    }
  });
}

test('knob overrange clamps to the experimental maximum and turning off discards the old tail', async () => {
  const effect = await open('dt2', 5);
  const clamped = await open('dt2', 0.8);
  try {
    let peak = 0;
    let lastPeak = 0;
    for (let k = 0; k < 240; k++) {
      const input = burst(k, 0.9);
      const got = await effect.block(input, 'friendly');
      const want = await clamped.block(input, 'friendly');
      expect(got.scratch).toEqual(want.scratch);
      lastPeak = Math.max(...got.scratch.map(Math.abs));
      peak = Math.max(peak, lastPeak);
    }
    // This intentionally hot burst may overshoot unity. It must stay inside
    // the effect contract, and its tail must shrink rather than run away.
    expect(peak).toBeLessThan(8);
    expect(lastPeak).toBeLessThan(peak / 2);
    effect.setRowWord(0, feedbackWord, 0);
    clamped.setRowWord(0, feedbackWord, 0);
    await effect.block(zero, 'friendly');
    await clamped.block(zero, 'friendly');
    effect.setRowWord(0, feedbackWord, f2w(0.78));
    for (let k = 0; k < delayFrames / blockSize; k++) {
      const got = await effect.block(zero, 'friendly');
      const off = await clamped.block(zero, 'friendly');
      const count = k * blockSize + blockSize <= delayFrames ? blockSize : blockSize / 2;
      expect(got.scratch.slice(0, count)).toEqual(off.scratch.slice(0, count));
    }
  } finally {
    await Promise.all([effect.close(), clamped.close()]);
  }
});

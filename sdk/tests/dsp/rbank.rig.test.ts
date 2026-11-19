/**
 * sdk/dsp resonator bank (`_dk_rbank`), its state-variable band (`_dk_svf_bp`, `_dk_svf_band`) and the step macros `DK_EXP2` and
 * `DK_TAN`, on the SHARC-only rig: one small harness object includes the SDK files and calls the routines the way a module does
 * (`DK_CALL`, the C convention of sdk/dsp/contract/call.inc).
 *
 * References: a float32 model of the same algorithm (smoothing, per-block coefficients, the operation order of the loop) with ideal
 * 2^x, tan and 1/x, so that the macros' errors show in the comparison; the model's loop on the coefficients the routine stored is
 * bit-exact against the routine; and a double-precision TPT filter (the direct form of svf.inc) for the response.
 */
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { PyRandom, RawRig, amplitudeAt, f2w, w2f } from '@digi/sharc-rig-kit';

const REPO = path.resolve(import.meta.dirname, '../../..');
const INCLUDES = [path.join(REPO, 'sdk/dsp/contract'), path.join(REPO, 'sdk')];
const f = Math.fround;
const FS = 48000;
const BLOCK = 32;

// DM layout (normal-word addresses, L2): the state, the three argument blocks, the audio buffers, the stack
const STATE = 0x08020000;
const STATE_WORDS = 104;
const PARAMS = 0x08020400;
const ARGS = 0x08020800;
const STACK = 0x08021000;
const IN = 0x08030000;
const OUT = 0x08040000;
const MARK = 0x5a5a5a5a;
const MODE1 = 0x10000; // RND32 on, TRUNCATE off, PEYEN off (the module contract)
const CONSTS = { M5: 0, M6: 1, M7: 0xffffffff, M14: 1, MODE1 };

const HARNESS = `
#include "dk_machine.inc"
#include "dsp/contract/call.inc"
#include "dsp/util/recip.inc"
#include "dsp/util/exp2.inc"
#include "dsp/util/tan.inc"
#include "dsp/filter/svf.inc"
#include "dsp/filter/rbank.inc"
.SECTION/PM seg_pmco;
.GLOBAL _h_rbank;
.GLOBAL _h_run;
.GLOBAL _h_bp;
.GLOBAL _h_exp2;
.GLOBAL _h_tan;
_h_rbank:
    DK_CALL(_dk_rbank, _h_rbank_after);
    RTS;
_h_bp:
    DK_CALL(_dk_svf_bp, _h_bp_after);
    RTS;
// R13 blocks in a row: the state, the params and the args block at fixed addresses; the args block's two pointers advance by 32
_h_run:
_h_run_loop:
    R4 = ${STATE};
    R8 = ${ARGS};
    R12 = ${PARAMS};
    DK_CALL(_dk_rbank, _h_run_after);
    I4 = ${ARGS};
    R0 = DM(0, I4);
    R1 = DM(1, I4);
    R2 = 32;
    R0 = R0 + R2;
    R1 = R1 + R2;
    DM(0, I4) = R0;
    DM(1, I4) = R1;
    R3 = 1;
    R13 = R13 - R3;
    IF NE JUMP _h_run_loop;
    RTS;
// R15 values from I4 to I0
_h_exp2:
    LCNTR = R15, DO _h_exp2_end UNTIL LCE;
        F0 = DM(I4, M6);
        DK_EXP2(F1, F0, R2, F3, R4, F4);
_h_exp2_end: DM(I0, M6) = F1;
    RTS;
_h_tan:
    LCNTR = R15, DO _h_tan_end UNTIL LCE;
        F0 = DM(I4, M6);
        DK_TAN(F1, F0, F2, F3, F5, R4, F4);
_h_tan_end: DM(I0, M6) = F1;
    RTS;
`;

// ---------------------------------------------------------------------------------------------------------------------------
// the model

/** The float32 value of an immediate of the routine (the hex words in rbank.inc). */
const K = (hex: number): number => f(w2f(hex));
const W01 = K(0x38010204); // 1 / 32512
const ALPHA = K(0x3e9122cf); // 1 - exp(-1/3)
const L250 = K(0x40fee7b4);
const XOFF = K(0xc133a93e);
const SEVEN = 7;
const NLQ = K(0xc0c511da);
const QOFF = K(0x3f03bb11);
const HI = K(0x3ee66666); // 0.45
const LO = K(0x39da740e); // 20 / 48000

interface Record7 {
  ic1: number;
  ic2: number;
  ka1: number;
  e1: number;
  a2d: number;
  ka2: number;
  a3d: number;
}

/** One band over a block: the six products and six adds of the loop in their order, each rounded to float32; `out` gets the sum. */
function bandLoop(r: Record7, x: ArrayLike<number>, out: Float32Array): { ic1: number; ic2: number } {
  let { ic1, ic2 } = r;
  for (let n = 0; n < BLOCK; n++) {
    const v3 = f(x[n]! - ic2);
    const p2 = f(r.a2d * ic1);
    const p1 = f(r.ka1 * ic1);
    const p3 = f(r.e1 * ic1);
    const q3 = f(v3 * r.a2d);
    const q2 = f(v3 * r.a3d);
    const q1 = f(v3 * r.ka2);
    const y = f(p1 + q1);
    out[n] = f(y + out[n]!);
    const nic1 = f(p3 + q3);
    const t = f(p2 + q2);
    ic2 = f(ic2 + t);
    ic1 = nic1;
  }
  return { ic1, ic2 };
}

/** The coefficients of svf.inc from g and k, in the order of DK_SVF_COEFS, with an ideal 1 / d. */
function svfCoefs(g: number, k: number, ks: number = k): Omit<Record7, 'ic1' | 'ic2'> {
  const t = f(g * f(g + k));
  const d = f(t + 1);
  const a1 = f(1 / d);
  const a2d0 = f(g * a1);
  const a2d = f(a2d0 + a2d0);
  const a3d = f(g * a2d);
  const ka1 = f(ks * a1);
  const ka2 = f(ka1 * g);
  const e1 = f(f(a1 + a1) - 1);
  return { ka1, e1, a2d, ka2, a3d };
}

class BankModel {
  first = true;
  ps = [0, 0, 0];
  bands: Record7[] = Array.from({ length: 8 }, () => ({ ic1: 0, ic2: 0, ka1: 0, e1: 0, a2d: 0, ka2: 0, a3d: 0 }));
  /** the coefficients of the last block  */
  used: Record7[] = [];

  block(x: ArrayLike<number>, words: [number, number, number]): Float32Array {
    const p = words.map((w) => Math.min(f(w * W01), 1));
    if (this.first) {
      this.ps = p;
      for (const b of this.bands) Object.assign(b, { ic1: 0, ic2: 0 });
      this.first = false;
    } else {
      this.ps = this.ps.map((s, i) => f(s + f(f(p[i]! - s) * ALPHA)));
    }
    const [pF, pS, pQ] = this.ps as [number, number, number];
    const x0 = f(f(f(pF * L250) - f(pS * SEVEN)) + XOFF);
    let cn = f(2 ** x0);
    const r = f(2 ** f(pS + pS));
    const k = f(2 ** f(f(pQ * NLQ) + QOFF));
    // the output gain 1 / S at the middle band, then ks = k / S
    const q2 = f(f(1 / k) * f(1 / k));
    const iu = f(1 / r);
    let [u, v] = [r, iu];
    const term = (): number => {
      const w = f(f(f(f(u - v) * f(u - v)) * q2) + 1);
      return f(1 / Math.sqrt(w));
    };
    let acc = term();
    for (let d = 2; d <= 3; d++) {
      u = f(u * r);
      v = f(v * iu);
      acc = f(acc + term());
    }
    acc = f(acc + acc);
    u = f(u * r);
    v = f(v * iu);
    const sum = f(f(acc + term()) + 1);
    const ks = f(k * f(1 / sum));
    const out = new Float32Array(BLOCK);
    this.used = [];
    this.bands.forEach((b, i) => {
      const cc = Math.max(cn, LO);
      const ct = Math.min(cc, HI);
      const g = f(Math.tan(Math.PI * ct));
      const c = svfCoefs(g, k, ks);
      cn = f(cn * r);
      Object.assign(b, c);
      this.used[i] = { ...b, ...c };
      Object.assign(b, bandLoop(b, x, out));
    });
    return out;
  }
}

/** Double-precision bank with the direct form of svf.inc and the ideal mapping of the knobs; no smoothing (steady knobs). */
class IdealBank {
  private readonly c: { a1: number; a2: number; a3: number; k: number; ic1: number; ic2: number }[] = [];
  readonly centres: number[] = [];
  /** S of rbank.inc in double precision */
  readonly norm: number;
  constructor(words: [number, number, number]) {
    const [p0, p1, p2] = words.map((w) => Math.min(w / 32512, 1)) as [number, number, number];
    const F = 20 * 250 ** p0;
    const s = 2 * p1;
    const Q = 0.7 * (50 / 0.7) ** p2;
    const u = 2 ** s;
    this.norm = [0, 1, 2, 3, 4].reduce((a, d) => a + (d === 0 || d === 4 ? 1 : 2) / Math.sqrt(1 + Q * Q * (u ** d - u ** -d) ** 2), 0);
    for (let i = 0; i < 8; i++) {
      const fc = Math.min(Math.max(F * 2 ** (s * (i - 3.5)), 20), 0.45 * FS);
      this.centres.push(fc);
      const g = Math.tan((Math.PI * fc) / FS);
      const k = 1 / Q;
      const a1 = 1 / (1 + g * (g + k));
      this.c.push({ a1, a2: g * a1, a3: g * g * a1, k: k / this.norm, ic1: 0, ic2: 0 });
    }
  }
  run(x: ArrayLike<number>): number[] {
    const y = Array.from<number>({ length: x.length }).fill(0);
    for (const b of this.c)
      for (let n = 0; n < x.length; n++) {
        const v3 = x[n]! - b.ic2;
        const v1 = b.a1 * b.ic1 + b.a2 * v3;
        const v2 = b.ic2 + b.a2 * b.ic1 + b.a3 * v3;
        b.ic1 = 2 * v1 - b.ic1;
        b.ic2 = 2 * v2 - b.ic2;
        y[n] = y[n]! + b.k * v1;
      }
    return y;
  }
}

const word = (p: number): number => Math.round(p * 0x7f00);
const knobs = (p: [number, number, number]): [number, number, number] => [word(p[0]), word(p[1]), word(p[2])];

// ---------------------------------------------------------------------------------------------------------------------------
// the rig

let rig: RawRig;

beforeAll(async () => {
  rig = await RawRig.open({
    objects: [{ text: HARNESS, includes: INCLUDES, base: 0xb90000 }],
    returnSw: 0xb9f000,
  });
}, 120_000);

afterAll(async () => {
  await rig?.close();
});

const SAVED = ['R3', 'R5', 'R6', 'R7', 'R9', 'R10', 'R11', 'R13', 'R14', 'R15', 'I0', 'I1', 'I2', 'I3', 'I5', 'M1', 'M2', 'M3', 'B3'];
const poison = (): Record<string, number> =>
  Object.fromEntries(SAVED.map((r, i) => [r, r[0] === 'I' || r === 'B3' ? 0x08021100 + 0x10 * i : 0x7000 + i]));
const KEPT_OK = ['I6', 'I7', 'M5', 'M6', 'M7', 'M14', 'L0', 'L1', 'L2', 'L3', 'L4', 'L5', 'MODE1'];

const poke = (address: number, words: readonly number[]): void => words.forEach((w, i) => rig.poke(address + i, w));
const pokeF = (address: number, x: ArrayLike<number>): void => {
  for (let i = 0; i < x.length; i++) rig.poke(address + i, f2w(x[i]!));
};

function resetState(garbage = false): void {
  const g = new PyRandom(99);
  for (let i = 0; i < STATE_WORDS; i++) rig.poke(STATE + i, garbage && i > 0 ? f2w(g.uniform(-5, 5)) : 0);
}

/** The state words as floats (words 0 .. 103). */
const stateWords = async (): Promise<number[]> => (await rig.peek(STATE, STATE_WORDS)).map((w) => w ?? 0);
const recordOf = (words: number[], i: number): Record7 => {
  const [ic1, ic2, ka1, e1, a2d, ka2, a3d] = words.slice(8 + 8 * i, 15 + 8 * i).map((w) => f(w2f(w)));
  return { ic1: ic1!, ic2: ic2!, ka1: ka1!, e1: e1!, a2d: a2d!, ka2: ka2!, a3d: a3d! };
};

/** One block through `_dk_rbank` called as `_h_rbank` (the input at IN, the output at OUT). */
async function oneBlock(x: ArrayLike<number>, words: [number, number, number], extra: Record<string, number> = {}) {
  pokeF(IN, x);
  poke(PARAMS, words);
  poke(ARGS, [IN, OUT]);
  const res = await rig.call(
    '_h_rbank',
    { ...CONSTS, I6: STACK, I7: STACK, R4: STATE, R8: ARGS, R12: PARAMS, ...extra },
    { reads: [[OUT, BLOCK]], readRegs: [...KEPT_OK, ...SAVED] },
  );
  expect(res.halt).toBe('breakpoint');
  return { res, out: (res.reads[0] ?? []).map((w) => f(w2f(w ?? 0))) };
}

/** `n` blocks in a row (n a multiple of 1 .. 256), the input already poked at IN; the output as floats. */
async function runBlocks(n: number, words: [number, number, number]): Promise<{ out: number[]; instructions: number }> {
  poke(PARAMS, words);
  poke(ARGS, [IN, OUT]);
  const res = await rig.call(
    '_h_run',
    { ...CONSTS, I6: STACK, I7: STACK, R13: n },
    { reads: [[OUT, n * BLOCK]], maxSteps: 4_000_000 * Math.ceil(n / 256) },
  );
  expect(res.halt).toBe('breakpoint');
  return { out: (res.reads[0] ?? []).map((w) => f(w2f(w ?? 0))), instructions: res.instructions };
}

const sine = (fc: number, n: number, amp: number, start = 0): number[] =>
  Array.from({ length: n }, (_, i) => f(amp * Math.sin((2 * Math.PI * fc * (start + i)) / FS)));

const maxAbs = (a: readonly number[]): number => a.reduce((m, v) => Math.max(m, Math.abs(v)), 0);

// ---------------------------------------------------------------------------------------------------------------------------

describe('sdk/dsp resonator bank on the rig', () => {
  it('matches the float32 model and the loop is bit-exact on the stored coefficients', async () => {
    const g = new PyRandom(7);
    const settings: [number, number, number][] = [
      [0.45, 0.15, 0.4],
      [0.6, 0.5, 0.3],
      [0.5, 0.0, 0.6],
      [1.0, 0.4, 0.5],
    ];
    let worstModel = 0;
    for (const p of settings) {
      resetState(true);
      const model = new BankModel();
      for (let b = 0; b < 48; b++) {
        // a moving knob (smoothing in every block) and noise plus a sine
        const words = knobs([Math.min(1, p[0] + 0.004 * b), p[1], Math.max(0, p[2] - 0.003 * b)]);
        const x = Array.from({ length: BLOCK }, (_, n) => f(0.3 * g.uniform(-1, 1) + 0.2 * Math.sin((2 * Math.PI * 440 * (b * BLOCK + n)) / FS)));
        const before = await stateWords();
        const { out, res } = await oneBlock(x, words);
        expect(res.final_pc).toBe(0xb9f000);
        const want = model.block(x, words);
        worstModel = Math.max(worstModel, ...out.map((v, n) => Math.abs(v - want[n]!)));
        // the loop alone: the records before the block give ic1, ic2; the records after give the coefficients the block used
        const after = await stateWords();
        const exact = new Float32Array(BLOCK);
        for (let i = 0; i < 8; i++) {
          const c = recordOf(after, i);
          if (c.ka1 === 0) continue;
          const r0 = recordOf(before, i);
          const rec = b === 0 ? { ...c, ic1: 0, ic2: 0 } : { ...c, ic1: r0.ic1, ic2: r0.ic2 };
          const end = bandLoop(rec, x, exact);
          expect(f(w2f(after[8 + 8 * i]!)), `band ${i} ic1 after block ${b}`).toBe(end.ic1);
          expect(f(w2f(after[9 + 8 * i]!)), `band ${i} ic2 after block ${b}`).toBe(end.ic2);
        }
        expect(Array.from(exact), `block ${b} of ${p}`).toEqual(out);
      }
    }
    console.log(`model comparison: max |rig - float32 model| over all samples = ${worstModel.toExponential(2)}`);
    expect(worstModel).toBeLessThan(2e-5); // measured 7.8e-6: the macros' errors (about 1e-6 on a coefficient)
  }, 120_000);

  it('smooths a knob step by 1 - exp(-1/3) per block (2 ms at 1500 blocks per second)', async () => {
    // derived here, not from the constants of rbank.inc: 48000 / 32 blocks per second, time constant 2 ms
    const alpha = 1 - Math.exp(-1 / 1500 / 0.002);
    const from = [0.1, 0.2, 0.3];
    const to = [0.9, 0.8, 0.7];
    const x = Array.from<number>({ length: BLOCK }).fill(0);
    resetState(true);
    await oneBlock(x, knobs(from as [number, number, number]));
    const pw = (p: number): number => word(p) / 0x7f00;
    const first = (await stateWords()).slice(1, 4).map((w) => w2f(w));
    first.forEach((v, i) => expect(Math.abs(v - pw(from[i]!))).toBeLessThan(1e-6));
    for (let n = 1; n <= 8; n++) {
      await oneBlock(x, knobs(to as [number, number, number]));
      const got = (await stateWords()).slice(1, 4).map((w) => w2f(w));
      got.forEach((v, i) => {
        const want = pw(to[i]!) - (pw(to[i]!) - pw(from[i]!)) * (1 - alpha) ** n;
        expect(Math.abs(v - want), `knob ${i}, block ${n}`).toBeLessThan(2e-6);
      });
    }
  });

  it('maps FREQ, SPRD and Q to 20 Hz .. 5 kHz, 0 .. 2 octaves and Q 0.7 .. 50 at the knob extremes', async () => {
    const x = Array.from<number>({ length: BLOCK }).fill(0);
    const cases: { p: [number, number, number]; centres: number[]; q: number }[] = [
      { p: [0, 0, 0], centres: Array.from<number>({ length: 8 }).fill(20), q: 0.7 },
      { p: [1, 0, 1], centres: Array.from<number>({ length: 8 }).fill(5000), q: 50 },
      { p: [0, 1, 0], centres: [20, 20, 20, 20, 40, 160, 640, 2560], q: 0.7 },
      { p: [1, 1, 1], centres: [5000 / 128, 5000 / 32, 5000 / 8, 5000 / 2, 10000, 40000, 160000, 640000].map((c) => Math.min(Math.max(c, 20), 21600)), q: 50 },
    ];
    for (const { p, centres, q } of cases) {
      resetState();
      await oneBlock(x, knobs(p));
      const words = await stateWords();
      // S of the middle band in double precision, from the spacing and Q alone
      const u = 2 ** (2 * p[1]);
      const S = [0, 1, 2, 3, 4].reduce((a, d) => a + (d === 0 || d === 4 ? 1 : 2) / Math.sqrt(1 + q * q * (u ** d - u ** -d) ** 2), 0);
      centres.forEach((c, i) => {
        const r = recordOf(words, i);
        const g = r.a3d / r.a2d; // a3d = g a2d
        const fc = (Math.atan(g) * FS) / Math.PI;
        expect(Math.abs(fc / c - 1), `knobs ${p}, band ${i}: ${fc} Hz, want ${c}`).toBeLessThan(5e-5);
        const k = (r.ka1 / ((r.e1 + 1) / 2)) * S; // ka1 = (k / S) a1, a1 = (e1 + 1) / 2
        expect(Math.abs(k * q - 1), `knobs ${p}, Q`).toBeLessThan(2e-5);
      });
    }
  });

  it('has its centre gain at the band centres and between them', async () => {
    const settings: [number, number, number][] = [
      [0.45, 0.15, 0.4],
      [0.6, 0.5, 0.3],
      [0.5, 0.0, 0.6],
      [1.0, 0.4, 0.5],
    ];
    const blocks = 512;
    const skip = 8192;
    let worstModel = 0;
    let worstIdeal = 0;
    let tones = 0;
    for (const p of settings) {
      const words = knobs(p);
      const ideal0 = new IdealBank(words);
      // the centres (a band above 0.45 fs sits at 0.45 fs) and the geometric means of neighbouring distinct centres
      const c = ideal0.centres;
      const freqs = [...c, ...c.slice(1).map((v, i) => Math.sqrt(v * c[i]!))];
      for (const fc of new Set(freqs)) {
        const x = sine(fc, blocks * BLOCK, 0.25);
        resetState();
        pokeF(IN, x);
        const { out } = await runBlocks(blocks, words);
        const model = new BankModel();
        const m: number[] = [];
        for (let b = 0; b < blocks; b++) m.push(...model.block(x.slice(b * BLOCK, (b + 1) * BLOCK), words));
        const ideal = new IdealBank(words).run(x);
        const [a, am, ai] = [out, m, ideal].map((y) => amplitudeAt(y, fc, skip)) as [number, number, number];
        worstModel = Math.max(worstModel, Math.abs(a - am) / 0.25);
        worstIdeal = Math.max(worstIdeal, Math.abs(a - ai) / 0.25);
        tones++;
        expect(Math.abs(a - am) / 0.25, `rig vs model at ${fc.toFixed(1)} Hz, knobs ${p}`).toBeLessThan(1e-4);
        expect(Math.abs(a - ai) / 0.25, `rig vs ideal at ${fc.toFixed(1)} Hz, knobs ${p}`).toBeLessThan(2e-4);
      }
    }
    console.log(`response: ${tones} tones; max gain difference (relative to the input amplitude) rig vs model ${worstModel.toExponential(2)}, rig vs double-precision ideal ${worstIdeal.toExponential(2)}`);
  }, 300_000);

  it('has a peak gain of about 1 at SPRD 0 and at wide spacing (output scaled by 1 / S)', async () => {
    const blocks = 512;
    const rows: string[] = [];
    for (const [label, p, lo, hi] of [
      ['SPRD 0, Q 9', [0.5, 0, 0.6], 0.99, 1.01],
      ['SPRD 1 octave, Q 9, no band clamped', [0.55, 0.5, 0.6], 0.7, 1.05],
      ['SPRD 1.2 octaves, Q 25, no band clamped', [0.6, 0.6, 0.8], 0.7, 1.05],
      // bands piled up at a clamp add up: three bands sit at 20 Hz here (an information row: no gain is claimed)
      ['SPRD 2 octaves, Q 9, bands 0 to 2 at 20 Hz', [0.3, 1, 0.6], 0, 10],
    ] as [string, [number, number, number], number, number][]) {
      const words = knobs(p);
      const bank = new IdealBank(words);
      let top = 0;
      for (const fc of bank.centres) {
        const x = sine(fc, blocks * BLOCK, 0.5);
        resetState();
        pokeF(IN, x);
        const { out } = await runBlocks(blocks, words);
        top = Math.max(top, amplitudeAt(out, fc, 8192) / 0.5);
      }
      rows.push(`${label}: S ${bank.norm.toFixed(4)}, largest gain at a band centre ${top.toFixed(4)}`);
      expect(top, label).toBeGreaterThan(lo);
      expect(top, label).toBeLessThan(hi);
    }
    console.log(`peak gain:\n  ${rows.join('\n  ')}`);
  }, 300_000);

  it('is stable at Q = 50 and the lowest FREQ: noise, impulses, then silence', async () => {
    const chunk = 256;
    const g = new PyRandom(2026);
    const stats: string[] = [];
    for (const sprd of [0, 0.3]) {
      const words = knobs([0, sprd, 1]);
      resetState();
      let peak = 0;
      let finite = true;
      // 10 s of white noise (amplitude 0.5) with a full-scale impulse in every 10th block
      const total = Math.ceil((10 * FS) / BLOCK / chunk) * chunk;
      for (let c = 0; c < total / chunk; c++) {
        const x = Array.from({ length: chunk * BLOCK }, () => f(g.uniform(-0.5, 0.5)));
        for (let b = 0; b < chunk; b += 10) x[b * BLOCK + 3] = 1;
        pokeF(IN, x);
        const { out } = await runBlocks(chunk, words);
        finite &&= out.every(Number.isFinite);
        peak = Math.max(peak, maxAbs(out));
      }
      // 3 s of a full-strength sine at the lowest centre (20 Hz): the bands ring at their peak gain (0.5 at SPRD 0)
      for (let c = 0; c < Math.ceil((3 * FS) / BLOCK / chunk); c++) {
        pokeF(IN, sine(20, chunk * BLOCK, 0.5, c * chunk * BLOCK));
        const { out } = await runBlocks(chunk, words);
        finite &&= out.every(Number.isFinite);
        peak = Math.max(peak, maxAbs(out));
      }
      // 20 s of silence: the state decays
      pokeF(IN, Array.from<number>({ length: chunk * BLOCK }).fill(0));
      const silent = Math.ceil((20 * FS) / BLOCK / chunk);
      let silentPeak = 0;
      for (let c = 0; c < silent; c++) {
        const { out } = await runBlocks(chunk, words);
        finite &&= out.every(Number.isFinite);
        silentPeak = maxAbs(out);
      }
      const state = await stateWords();
      let stateMax = 0;
      for (let i = 0; i < 8; i++) stateMax = Math.max(stateMax, Math.abs(f(w2f(state[8 + 8 * i]!))), Math.abs(f(w2f(state[9 + 8 * i]!))));
      stats.push(`SPRD ${sprd}: noise, impulse and 20 Hz sine peak ${peak.toFixed(4)}, after 20 s of silence output peak ${silentPeak.toExponential(2)} and largest state ${stateMax.toExponential(2)}`);
      expect(finite).toBe(true);
      expect(peak).toBeLessThan(8);
      expect(stateMax).toBeLessThan(1e-6);
      expect(silentPeak).toBeLessThan(1e-6);
    }
    console.log(`stability (Q 50, FREQ 20 Hz, 10 s noise at 0.5, impulses, 3 s sine at 20 Hz):\n  ${stats.join('\n  ')}`);
  }, 600_000);

  it('survives full-range FREQ, SPRD and Q jumps between blocks', async () => {
    const g = new PyRandom(5);
    const corners: [number, number, number][] = [
      [0, 0, 0],
      [1, 1, 1],
      [0, 0, 1],
      [1, 0, 0],
      [0, 1, 1],
      [1, 1, 0],
      [0.5, 0, 1],
    ];
    resetState();
    let peak = 0;
    let finite = true;
    let blockNo = 0;
    for (let run = 0; run < 28; run++) {
      const words = knobs(corners[run % corners.length]!);
      for (let b = 0; b < 12; b++) {
        const x = Array.from({ length: BLOCK }, (_, n) => f(0.25 * g.uniform(-1, 1) + 0.25 * Math.sin((2 * Math.PI * (60 + 40 * run) * (blockNo * BLOCK + n)) / FS)));
        const { out } = await oneBlock(x, words);
        finite &&= out.every(Number.isFinite);
        peak = Math.max(peak, maxAbs(out));
        blockNo++;
      }
    }
    console.log(`knob jumps: ${blockNo} blocks, peak ${peak.toFixed(4)}`);
    expect(finite).toBe(true);
    expect(peak).toBeLessThan(8);
  }, 120_000);

  it('keeps the registers and the memory of the contract', async () => {
    resetState(true);
    // sentinels around every buffer and above the stack
    const guards: [string, number, number][] = [
      ['state', STATE, STATE_WORDS],
      ['params', PARAMS, 3],
      ['args', ARGS, 2],
      ['in', IN, BLOCK],
      ['out', OUT, BLOCK],
    ];
    const g = new PyRandom(3);
    const x = Array.from({ length: BLOCK }, () => f(g.uniform(-0.5, 0.5)));
    for (const [, base, len] of guards) {
      for (let i = 1; i <= 64; i++) {
        rig.poke(base - i, MARK + i);
        rig.poke(base + len + i - 1, MARK - i);
      }
    }
    for (let i = 1; i <= 64; i++) rig.poke(STACK + i, MARK ^ i); // the frames grow down from STACK, which is written itself
    pokeF(IN, x);
    poke(PARAMS, knobs([0.5, 0.2, 0.5]));
    poke(ARGS, [IN, OUT]);
    const regs: Record<string, number> = { ...CONSTS, ...poison(), I6: STACK, I7: STACK, R4: STATE, R8: ARGS, R12: PARAMS };
    const before = { ...regs };
    const res = await rig.call('_h_rbank', regs, {
      readRegs: [...KEPT_OK, ...SAVED],
      reads: [
        [PARAMS, 3],
        [ARGS, 2],
        [IN, BLOCK],
        ...guards.flatMap(([, base, len]): [number, number][] => [
          [base - 64, 64],
          [base + len, 64],
        ]),
        [STACK + 1, 64],
      ],
    });
    expect(res.halt).toBe('breakpoint');
    for (const r of SAVED) expect(res.regs[r] === null ? null : res.regs[r]! >>> 0, r).toBe(before[r]! >>> 0);
    for (const r of ['I6', 'I7', 'M5', 'M6', 'M7', 'M14', 'MODE1']) expect(res.regs[r]! >>> 0, r).toBe(before[r]! >>> 0);
    for (const r of ['L0', 'L1', 'L2', 'L3', 'L4', 'L5']) expect(res.regs[r]! >>> 0, r).toBe(0);
    expect(res.reads[0]!.map((w) => w ?? 0)).toEqual(knobs([0.5, 0.2, 0.5]));
    expect(res.reads[1]).toEqual([IN, OUT]);
    expect(res.reads[2]!.map((w) => f(w2f(w ?? 0)))).toEqual(x);
    guards.forEach(([name, , len], gi) => {
      expect(res.reads[3 + 2 * gi]!, `below ${name}`).toEqual(Array.from({ length: 64 }, (_, i) => MARK + (64 - i)));
      expect(res.reads[4 + 2 * gi]!, `above ${name} (${len} words)`).toEqual(Array.from({ length: 64 }, (_, i) => (MARK - (i + 1)) >>> 0));
    });
    expect(res.reads[3 + 2 * guards.length]!, 'above the stack').toEqual(Array.from({ length: 64 }, (_, i) => (MARK ^ (i + 1)) >>> 0));
  });

  it('runs a band alone as _dk_svf_bp, adding to the output block, bit-exact to the model loop', async () => {
    const g = new PyRandom(11);
    const gg = f(Math.tan((Math.PI * 1000) / FS));
    const k = f(1 / 4);
    const c = svfCoefs(gg, k);
    const rec = [0.2, -0.1, c.ka1, c.e1, c.a2d, c.ka2, c.a3d, 0].map((v) => f(v));
    const x = Array.from({ length: BLOCK }, () => f(g.uniform(-1, 1)));
    const base = Array.from({ length: BLOCK }, () => f(g.uniform(-1, 1)));
    for (let pass = 0; pass < 2; pass++) {
      poke(STATE, rec.map(f2w));
      pokeF(IN, x);
      pokeF(OUT, base);
      poke(ARGS, [IN, OUT]);
      const res = await rig.call(
        '_h_bp',
        { ...CONSTS, I6: STACK, I7: STACK, R4: STATE, R8: ARGS, R12: 0 },
        { reads: [[OUT, BLOCK], [STATE, 2]] },
      );
      expect(res.halt).toBe('breakpoint');
      const out = Float32Array.from(base);
      const end = bandLoop({ ic1: rec[0]!, ic2: rec[1]!, ka1: rec[2]!, e1: rec[3]!, a2d: rec[4]!, ka2: rec[5]!, a3d: rec[6]! }, x, out);
      expect(res.reads[0]!.map((w) => f(w2f(w ?? 0)))).toEqual(Array.from(out));
      expect(res.reads[1]!.map((w) => f(w2f(w ?? 0)))).toEqual([end.ic1, end.ic2]);
      if (pass === 0) console.log(`_dk_svf_bp: ${res.instructions - 1} instructions with DK_CALL (one band, one block)`);
    }
  });

  it('DK_EXP2 and DK_TAN: maximum relative error over their ranges', async () => {
    const run = async (name: string, xs: number[]): Promise<number[]> => {
      const out: number[] = [];
      for (let i = 0; i < xs.length; i += 2048) {
        const part = xs.slice(i, i + 2048);
        pokeF(IN, part);
        const res = await rig.call(
          name,
          { ...CONSTS, I6: STACK, I7: STACK, I4: IN, I0: OUT, R15: part.length },
          { reads: [[OUT, part.length]] },
        );
        expect(res.halt).toBe('breakpoint');
        out.push(...res.reads[0]!.map((w) => w2f(w ?? 0)));
      }
      return out;
    };
    // 2^x on -30 .. 30, dense, including the integers and the half-integers
    const xe = Array.from({ length: 6000 }, (_, i) => f(-30 + (60 * i) / 5999));
    xe.push(...[-16, -8.5, -0.5, 0, 0.5, 1, 7.5, 16].map(f));
    const ye = await run('_h_exp2', xe);
    const errE = Math.max(...xe.map((x, i) => Math.abs(ye[i]! / 2 ** x - 1)));
    // tan(pi x) on 0 .. 0.45
    const xt = Array.from({ length: 6000 }, (_, i) => f((0.45 * i) / 5999));
    const yt = await run('_h_tan', xt);
    const errT = Math.max(...xt.slice(1).map((x, i) => Math.abs(yt[i + 1]! / Math.tan(Math.PI * x) - 1)));
    console.log(`DK_EXP2 max relative error over -30 .. 30: ${errE.toExponential(2)}; DK_TAN over (0, 0.45]: ${errT.toExponential(2)}`);
    expect(errE).toBeLessThan(2.5e-7);
    expect(errT).toBeLessThan(1.3e-6);
  });

  it('costs at most 2,500 instructions in a steady block', async () => {
    const x = Array.from({ length: BLOCK }, (_, n) => f(0.3 * Math.sin(n)));
    const rows: string[] = [];
    let steadyWorst = 0;
    for (const [label, p] of [
      ['8 active bands', [0.45, 0.15, 0.4]],
      ['8 active bands, knobs moving', [0.2, 0.0, 0.9]],
      ['bands 6 and 7 clamped at 0.45 fs', [1.0, 0.4, 0.5]],
    ] as [string, [number, number, number]][]) {
      resetState(true);
      const first = await oneBlock(x, knobs(p));
      const second = await oneBlock(x, knobs([Math.min(1, p[0] + 0.1), p[1], p[2]]));
      const third = await oneBlock(x, knobs([p[0], p[1], p[2]]));
      // the harness adds DK_CALL's 3 instructions to the routine's count and one RTS
      const n = [first, second, third].map((r) => r.res.instructions - 1);
      rows.push(`${label}: first block ${n[0]}, steady blocks ${n[1]} and ${n[2]} (with DK_CALL)`);
      steadyWorst = Math.max(steadyWorst, n[1]!, n[2]!);
    }
    console.log(`cost, instructions per block:\n  ${rows.join('\n  ')}`);
    expect(steadyWorst).toBeLessThanOrEqual(2500);
  });
});

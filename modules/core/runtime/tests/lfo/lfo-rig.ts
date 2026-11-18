import { encoder as encoderCode } from '@digi/test-support';
import { expect } from 'vitest';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { bounded, REDRAW, SETTLE, word, type Rt } from '@digi/regression/runtime/fx-live.ts';
import { halfword } from '@digi/regression/runtime/fx-track-ui.ts';

/**
 * Helpers of the LFO shape device tests (plan E6): the LFO page of track 1 driven from the panel, the Sound words read from firmware memory, and a phase-free fit of the
 * LFO output against a shape.
 *
 * What the tests assume of the firmware (finding 143; each is checked on the way and fails with a message, so a wrong assumption shows at the first device run):
 * - the MOD key opens the page of LFO 1 with the knobs A to H = SPD, MULT, FADE, DEST, WAVE, SPH, MODE, DEPTH (the order of the parameter codes 75 to 83);
 * - the Sound words of LFO 1 are the halfwords 1 to 8 of the Sound array of track 1 in that order (WAVE is word 5, `locate` checks it on DT2);
 * - the LFO adds its output to the Sound word of its destination, so the destination word varies over time (`modulated` finds it by sampling, it is never named here);
 * - a speed of zero stops the LFO.
 */

export type Knob = 'SPD' | 'DEST' | 'WAVE' | 'SPH' | 'MODE' | 'DEPTH';
const KNOB_ENCODER: Record<Knob, string> = {
  SPD: 'ENC A',
  DEST: 'ENC D',
  WAVE: 'ENC E',
  SPH: 'ENC F',
  MODE: 'ENC G',
  DEPTH: 'ENC H',
};
/** The Sound word of each knob of LFO 1 on DT2 (finding 143, "The tick"); DN2 is found by `locate`. */
const DT2_WORD: Record<Knob, number> = { SPD: 1, DEST: 4, WAVE: 5, SPH: 6, MODE: 7, DEPTH: 8 };

/** The wave numbers of the stock waves (finding 143, "Waveform names"). */
export const WAVE = { TRI: 0, SINE: 1, SQR: 2, SAW: 3, EXPO: 4, RAMP: 5, RAND: 6 } as const;
/** The first wave number the lfo service gives a shape (`RANGE.lo` of the service). */
export const FIRST_SHAPE = 7;

/** The 32 samples of `tests/fixtures/modules/lfo-shape/src/module.c` (RAMP): a ramp from -1000 in steps of 100. */
export const RAMP_SAMPLES = Array.from({ length: 32 }, (_, i) => -1000 + 100 * i);
/** The samples of `tests/fixtures/modules/lfo-shape-b/src/module.c` (STEP). */
export const STEP_SAMPLES = Array.from({ length: 32 }, (_, i) => (i < 16 ? -800 : 800));

// ---- the waves as functions of a 32-bit phase, signed Q31 (finding 143, "Output scale") --------------------------------------------------

/** The value of a 32-sample table at `phase`: the formula of the service's `records::interp` (the sample at the top 5 bits, linear with the next 15). */
export function evalTable(s: readonly number[], phase: number): number {
  const i = phase >>> 27;
  const j = (i + 1) & 31;
  const f = (phase >>> 12) & 0x7fff;
  return ((s[i]! << 16) + (((s[j]! - s[i]!) * f) << 1)) | 0;
}
export const evalTri = (phase: number): number => {
  const d = (phase + 0x40000000) >>> 0;
  const mask = d >>> 31 ? 0xffffffff : 0;
  return ((d << 1) ^ mask ^ 0x80000000) | 0;
};
export const evalSqr = (phase: number): number => (phase >>> 0 < 0x80000000 ? 0x7fffffff : -0x80000000);
export const evalSaw = (phase: number): number => (phase ^ 0x7fffffff) | 0;
export const evalRamp = (phase: number): number => (phase >>> 0 < 0x80000000 ? phase | 0 : 0);
/** The stock sine is a polynomial; the fit tolerance covers its difference to the true sine. */
export const evalSine = (phase: number): number => Math.round(Math.sin((phase / 2 ** 32) * 2 * Math.PI) * 0x7fffffff);
export const STOCK_EVAL: Record<number, (phase: number) => number> = {
  [WAVE.TRI]: evalTri,
  [WAVE.SINE]: evalSine,
  [WAVE.SQR]: evalSqr,
  [WAVE.SAW]: evalSaw,
  [WAVE.RAMP]: evalRamp,
};

// ---- a phase-free fit ----------------------------------------------------------------------------------------------------------------

export interface Sample {
  /** Emulated ms. */
  t: number;
  /** The Sound word. */
  v: number;
}

export interface Fit {
  /** Root mean square difference of the normalised samples to the normalised shape, 0 to about 0.5. */
  err: number;
  /** Cycles per emulated ms and the phase (0 to 1) at the first sample of the best fit. */
  f: number;
  phi: number;
}

const PHASES = 512;
const FIT_CYCLES_PER_MS: [number, number] = [1 / 2000, 1 / 24];
const FIT_RATES = 240;
const FIT_OFFSETS = 128;

/**
 * Fit the samples to `shape`: the LFO output is `base + scale * shape(phase)` with a phase that grows at a constant rate, and none of base, scale, rate or start phase is known.
 * Both curves are normalised to 0..1 by their own minimum and maximum, then the rate (log steps) and the start phase are searched for the smallest root mean square
 * difference. A shape that is not the one that made the samples leaves an error of 0.1 or more at 8 to 16 samples per cycle over 2 or more cycles; the true shape leaves
 * the quantisation of the Sound words and the jitter of the sample times (0.05 or less).
 */
export function fitShape(samples: readonly Sample[], shape: (phase: number) => number): Fit {
  const vs = samples.map((s) => s.v);
  const lo = Math.min(...vs);
  const hi = Math.max(...vs);
  if (hi === lo) return { err: 1, f: 0, phi: 0 };
  const u = vs.map((v) => (v - lo) / (hi - lo));
  const table = Array.from({ length: PHASES }, (_, k) => shape(Math.floor((k / PHASES) * 2 ** 32) >>> 0));
  const tlo = Math.min(...table);
  const thi = Math.max(...table);
  const e = table.map((x) => (thi === tlo ? 0 : (x - tlo) / (thi - tlo)));
  const t0 = samples[0]!.t;
  let best: Fit = { err: Infinity, f: 0, phi: 0 };
  for (let r = 0; r < FIT_RATES; r++) {
    const f = FIT_CYCLES_PER_MS[0] * (FIT_CYCLES_PER_MS[1] / FIT_CYCLES_PER_MS[0]) ** (r / (FIT_RATES - 1));
    for (let o = 0; o < FIT_OFFSETS; o++) {
      const phi = o / FIT_OFFSETS;
      let sum = 0;
      for (let n = 0; n < samples.length; n++) {
        const cycle = phi + f * (samples[n]!.t - t0);
        const k = Math.floor((cycle - Math.floor(cycle)) * PHASES) % PHASES;
        const d = u[n]! - e[k]!;
        sum += d * d;
      }
      const err = Math.sqrt(sum / samples.length);
      if (err < best.err) best = { err, f, phi };
    }
  }
  return best;
}

/** Number of times the samples cross the middle of their range: 2 per cycle for every shape of the tests. */
export function crossings(samples: readonly Sample[]): number {
  const vs = samples.map((s) => s.v);
  const mid = (Math.min(...vs) + Math.max(...vs)) / 2;
  if (Math.max(...vs) === Math.min(...vs)) return 0;
  let n = 0;
  for (let i = 1; i < vs.length; i++) if (vs[i - 1]! < mid !== vs[i]! < mid) n++;
  return n;
}

// ---- the rig -------------------------------------------------------------------------------------------------------------------------

const SAMPLING = 'emulated time between two reads of the LFO output (no observable state)';
const GAP = 'the gap between encoder events (the stock encoder scales by event rate)';

/** Track 1, LFO 1, driven from the panel. Create it after the runtime is up; it reads the Sound words of track 1 from firmware memory. */
export class LfoRig {
  /** The Sound word of each knob, found by `locate`. */
  readonly index: Partial<Record<Knob, number>> = {};
  private soundBase: number | undefined;

  readonly rt: Rt;

  constructor(rt: Rt) {
    this.rt = rt;
  }

  private get session() {
    return this.rt.session;
  }
  private get device() {
    return this.session.options.device;
  }

  private async base(): Promise<number> {
    if (this.soundBase === undefined) {
      const { fx } = deviceProfile(this.device) as unknown as { fx: { kit_ptr: number; sound_off: number } };
      // the Sound array of track 1 starts 0x14 into its Sound object; the kit object holds the Sound objects from `sound_off` on
      this.soundBase = (await word(this.session, fx.kit_ptr)) + fx.sound_off + 0x14;
    }
    return this.soundBase;
  }

  /** Sound word `index` of track 1. */
  async word(index: number): Promise<number> {
    return halfword(this.session, (await this.base()) + 2 * index);
  }

  /** The whole Sound array of track 1. */
  async words(): Promise<number[]> {
    const { fx } = deviceProfile(this.device) as unknown as { fx: { sound_words: number } };
    return Promise.all(Array.from({ length: fx.sound_words }, (_, i) => this.word(i)));
  }

  /** The MOD page, LFO 1. */
  async open(): Promise<void> {
    await this.rt.ui.tap('MOD', { until: { ms: 300, why: REDRAW } });
  }

  /** `count` events of `step` detents, then a settle for the firmware to store the last one. */
  async turn(knob: Knob, step: number, count = 1, gapMs = 40): Promise<void> {
    for (let i = 0; i < count; i++) {
      await bounded(() => this.session.client.inputTurn(encoderCode(KNOB_ENCODER[knob]), step));
      await this.rt.ui.wait({ ms: gapMs, why: GAP });
    }
    await this.rt.ui.wait({ ms: 100, why: SETTLE });
  }

  /** The Sound word of `knob`: turn it up in bursts until exactly one word moves (the lead-in filter of the stock encoder swallows the first counts, finding 03). */
  async locate(knob: Knob): Promise<number> {
    const before = await this.words();
    for (let n = 0; n < 4; n++) {
      await this.turn(knob, 4);
      const after = await this.words();
      const moved = after.flatMap((v, i) => (v === before[i] ? [] : [i]));
      if (moved.length === 0) continue;
      expect(moved, `the ${knob} knob (${KNOB_ENCODER[knob]}) moves one Sound word`).toHaveLength(1);
      if (this.device === 'dt2') expect(moved[0], `${knob} is the Sound word the finding names`).toBe(DT2_WORD[knob]);
      this.index[knob] = moved[0]!;
      return moved[0]!;
    }
    throw new Error(`${KNOB_ENCODER[knob]} (${knob}) moved no Sound word in four bursts: is the LFO page open?`);
  }

  /** Turn `knob` one detent at a time until its word is within `tol` of `want`; fails after `max` events. A word that stops at a limit short of `want` fails too. */
  async turnTo(knob: Knob, want: number, tol = 0, max = 60): Promise<number> {
    const index = this.index[knob] ?? (await this.locate(knob));
    let stuck = 0;
    let last = -1;
    for (let n = 0; n < max; n++) {
      const got = await this.word(index);
      if (Math.abs(got - want) <= tol) return got;
      stuck = got === last ? stuck + 1 : 0;
      if (stuck >= 4) throw new Error(`${knob} stops at ${got.toString(16)}, wanted ${want.toString(16)}`);
      last = got;
      const far = Math.abs(want - got) > 0x1000;
      await this.turn(knob, (want > got ? 1 : -1) * (far ? 4 : 1), 1, far ? 4 : 40);
    }
    throw new Error(`${knob} did not reach ${want.toString(16)} in ${max} events (at ${(await this.word(index)).toString(16)})`);
  }

  /** WAVE to wave number `n`: the word is `n << 8`. Fails when the encoder stops short of it (the maximum of the parameter). */
  async setWave(n: number): Promise<void> {
    await this.turnTo('WAVE', n << 8);
  }

  /** The Sound words that vary while the LFO runs (the destination of the LFO), not counting the knob words. */
  async modulated(): Promise<number[]> {
    const knobWords = new Set(Object.values(this.index));
    const seen: number[][] = [];
    for (let n = 0; n < 6; n++) {
      seen.push(await this.words());
      await this.rt.ui.wait({ ms: 7, why: SAMPLING });
    }
    return seen[0]!.flatMap((_, i) => (!knobWords.has(i) && seen.some((w) => w[i] !== seen[0]![i]) ? [i] : []));
  }

  /** `count` reads of Sound word `index`, `gapMs` of emulated time apart, with the emulated time of each. */
  async sample(index: number, count: number, gapMs: number): Promise<Sample[]> {
    const out: Sample[] = [];
    for (let n = 0; n < count; n++) {
      out.push({ v: await this.word(index), t: await this.rt.clock.now() });
      await this.rt.ui.wait({ ms: gapMs, why: SAMPLING });
    }
    return out;
  }

  /**
   * Make the LFO run into one destination at a speed the fit can follow: locate the knobs (DEPTH last, so nothing is modulated while they are found), choose the
   * first destination that makes a Sound word vary, raise the depth, and adjust the speed until a window of 40 reads at 12 ms holds 6 to 14 crossings of its middle.
   * `known`: the knob words of another rig on the same device, so that no knob is turned to find them (a project that holds a wave must keep it). Returns the index of the
   * destination word.
   */
  async run(known?: Partial<Record<Knob, number>>): Promise<number> {
    await this.open();
    Object.assign(this.index, known);
    for (const knob of ['DEST', 'SPD', 'WAVE', 'SPH', 'MODE', 'DEPTH'] as const) if (this.index[knob] === undefined) await this.locate(knob);
    await this.turnTo('DEPTH', 0x3f00, 0x1f00); // about half of the range: the sum with the destination's own word stays inside 0 to 0x7f00
    let dest = -1;
    for (let d = 1; d <= 6 && dest < 0; d++) {
      await this.turnTo('DEST', d << 8);
      const moving = await this.modulated();
      if (moving.length === 1) dest = moving[0]!;
      else if (moving.length > 1) throw new Error(`DEST ${d} moves several Sound words: ${moving.join(',')}`);
    }
    if (dest < 0) throw new Error('no destination of DEST 1 to 6 makes a Sound word vary: the LFO output is not in the Sound array of track 1, or SPD 0 stops it');
    await this.tuneSpeed(dest);
    return dest;
  }

  /** Speed so that 40 reads at 12 ms hold 6 to 14 crossings (3 to 7 cycles, 6 to 13 reads per cycle). */
  async tuneSpeed(dest: number): Promise<void> {
    for (let n = 0; n < 12; n++) {
      const c = crossings(await this.sample(dest, 40, 12));
      if (c >= 6 && c <= 14) return;
      await this.turn('SPD', c < 6 ? 4 : -4, 1, 40);
    }
    throw new Error('the LFO speed could not be set to 3 to 7 cycles in 0.5 s of emulated time');
  }

  /** Stop the LFO (a speed of 0): the speed knob is moved one detent at a time over the middle of its range until 4 reads of the destination 40 ms apart agree. */
  async freeze(dest: number): Promise<void> {
    const speed = this.index.SPD!;
    const isStatic = async () => {
      const reads = await this.sample(dest, 4, 40);
      return reads.every((s) => s.v === reads[0]!.v);
    };
    // the speed knob has a signed range: walk it to its two ends to find the middle
    await this.turn('SPD', 4, 40, 4);
    const top = await this.word(speed);
    await this.turn('SPD', -4, 80, 4);
    const bottom = await this.word(speed);
    const middle = Math.round((top + bottom) / 2);
    await this.turnTo('SPD', middle, 0x100);
    for (const step of [0, -1, 1, -1, 1, -1, 1]) {
      if (step !== 0) await this.turn('SPD', step, 1, 40);
      if (await isStatic()) return;
    }
    throw new Error(`no speed near the middle ${middle.toString(16)} (range ${bottom.toString(16)} to ${top.toString(16)}) stops the LFO`);
  }
}

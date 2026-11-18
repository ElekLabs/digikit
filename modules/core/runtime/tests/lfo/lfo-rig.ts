import { encoder as encoderCode, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { REDRAW, SETTLE, word, type Rt } from '@digi/regression/runtime/fx-live.ts';
import { halfword } from '@digi/regression/runtime/fx-track-ui.ts';
import { crossings, swing, type Sample } from './lfo-fit.ts';

export * from './lfo-fit.ts';

/**
 * Helpers of the LFO shape device tests (plan E6): the LFO page of track 1 driven from the panel, the knob words read from the Sound object, the LFO output read from the
 * array the tick writes it into, and a phase-free fit of the output against a shape.
 *
 * What the tests assume of the firmware (finding 143, "Where the output lands" and "The knob words"; each is checked on the way and fails with a message, so a wrong
 * assumption shows at the first device run):
 * - the MOD key opens the page of LFO 1 with the knobs A to H = SPD, MULT, FADE, DEST, WAVE, SPH (SLEW for RAND), MODE, DEPTH (the order of the parameter codes 75 to 83);
 * - the knob words of LFO 1 are the halfwords 1 to 8 of the Sound array of track 1 in that order (`locate` checks each against `KNOB_WORD` on both devices);
 * - the LFO does not write the Sound object: the tick adds its output to the destination word of the smoothed mirror that the DSP frame is built from (`LIVE`), so the
 *   output is read there (`live`), and the knob words are read from the Sound object (`word`);
 * - SPD and DEPTH are bipolar around 0x4000: a speed of 0x4000 stops the LFO, and a depth of 0x4000 gives no output.
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
/** The word of each knob of LFO 1 in the Sound array of track 1, the same on both devices (finding 143, "The knob words"); `locate` checks it. */
const KNOB_WORD: Record<Knob, number> = { SPD: 1, DEST: 4, WAVE: 5, SPH: 6, MODE: 7, DEPTH: 8 };

/**
 * The array of track 1 that the LFO tick adds its output to (finding 143, "Where the output lands"): the smoothed mirror that the vector 191 handler builds from the
 * Sound array. Word i of the Sound array is at `base + 2 * i` (track t at `base + stride * t`); `base` is the return value of the smoother plus 0x22 (DT2 0x80005b50,
 * DN2 0x800068e4). The smoother rewrites the array on every handler run and the tick adds the output after it, so a word that an LFO targets reads
 * smoothed base word + output, clamped to 0..0x7f00. `words` is the length of a row (the stride in halfwords).
 */
const LIVE: Record<Device, { base: number; stride: number; words: number }> = {
  dt2: { base: 0x80005b72, stride: 0x8e, words: 71 },
  dn2: { base: 0x80006906, stride: 0xca, words: 101 },
};
/** The phase accumulator (u32) of LFO 1 of track 1 in the tick's state (finding 143): it moves with every tick of a running LFO; used for the failure message only. */
const LFO_PHASE: Record<Device, number> = { dt2: 0x44e66e78, dn2: 0x44640c18 };
/**
 * Array indices tried as destination, in order. Each has a mid-range stock default (0x4000, or 0x2000), so that the output does not clip at 0 or 0x7f00; none is a word
 * of an LFO (1 to 24). The DEST word is `index << 8` (the tick reads its high byte as the array index, limit 0x46 on DT2 and 0x64 on DN2).
 */
const DEST_CANDIDATES = [34, 26, 35, 36, 38, 39];
/** DEPTH is bipolar: the output amplitude is (depth - 0x4000) * 2 words times the shape. 0x5000 gives +-0x2000 around the base word. */
const DEPTH_WANT = 0x5000;
const DEPTH_ZERO = 0x4000;
/** The smallest peak-to-peak swing of the destination word that counts as an LFO output (the full swing at DEPTH_WANT is 0x4000 times the fade level). */
export const MIN_SWING = 0x400;

// ---- the rig -------------------------------------------------------------------------------------------------------------------------

const SAMPLING = 'emulated time between two reads of the LFO output (no observable state)';
const GAP = 'the gap between encoder events (the stock encoder scales by event rate)';

/** Track 1, LFO 1, driven from the panel. Create it after the runtime is up; it reads the Sound words of track 1 from firmware memory. */
export class LfoRig {
  /** The Sound word of each knob, found by `locate` (or given by `known`). */
  readonly index: Partial<Record<Knob, number>> = {};
  private soundBase: number | undefined;

  readonly rt: Rt;

  /** `known`: the knob words of another rig on the same device, so that no knob is turned to find them (a project that holds a wave must keep it). */
  constructor(rt: Rt, known?: Partial<Record<Knob, number>>) {
    this.rt = rt;
    Object.assign(this.index, known);
  }

  private get session() {
    return this.rt.session;
  }
  private get device(): Device {
    return this.session.options.device;
  }

  private async base(): Promise<number> {
    if (this.soundBase === undefined) {
      const { fx } = deviceProfile(this.device);
      // the Sound array of track 1 starts 0x14 into its Sound object; the kit object holds the Sound objects from `sound_off` on
      this.soundBase = (await word(this.session, fx.kit_ptr)) + fx.sound_off + 0x14;
    }
    return this.soundBase;
  }

  /** Sound word `index` of track 1: the stored knob value (the LFO does not write it). */
  async word(index: number): Promise<number> {
    return halfword(this.session, (await this.base()) + 2 * index);
  }

  /** The Sound array of track 1. */
  async words(): Promise<number[]> {
    const { fx } = deviceProfile(this.device);
    return Promise.all(Array.from({ length: fx.sound_words }, (_, i) => this.word(i)));
  }

  /** Word `index` of the array the tick writes into (smoothed base + LFO output, see `LIVE`). */
  async live(index: number): Promise<number> {
    return halfword(this.session, LIVE[this.device].base + 2 * index);
  }

  /** The whole live row of track 1. */
  async liveWords(): Promise<number[]> {
    return Promise.all(Array.from({ length: LIVE[this.device].words }, (_, i) => this.live(i)));
  }

  /** The MOD page, LFO 1. */
  async open(): Promise<void> {
    await this.rt.ui.tap('MOD', { until: { ms: 300, why: REDRAW } });
  }

  /** `count` events of `step` detents, then a settle for the firmware to store the last one. */
  async turn(knob: Knob, step: number, count = 1, gapMs = 40): Promise<void> {
    for (let i = 0; i < count; i++) await this.rt.ui.turn(encoderCode(KNOB_ENCODER[knob]), step, { until: { ms: gapMs, why: GAP } });
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
      expect(moved[0], `${knob} is the Sound word the finding names`).toBe(KNOB_WORD[knob]);
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

  /** The words of the live row that vary over 6 reads 7 ms apart, not counting the knob words of this LFO. */
  async moving(): Promise<number[]> {
    const knobWords = new Set(Object.values(this.index));
    const seen: number[][] = [];
    for (let n = 0; n < 6; n++) {
      seen.push(await this.liveWords());
      await this.rt.ui.wait({ ms: 7, why: SAMPLING });
    }
    return seen[0]!.flatMap((_, i) => (!knobWords.has(i) && seen.some((w) => w[i] !== seen[0]![i]) ? [i] : []));
  }

  /** True when the phase accumulator of LFO 1 of track 1 moves between two reads 20 ms apart: the tick runs and the LFO is not stopped. */
  async phaseMoves(): Promise<boolean> {
    const first = await word(this.session, LFO_PHASE[this.device]);
    await this.rt.ui.wait({ ms: 20, why: SAMPLING });
    return (await word(this.session, LFO_PHASE[this.device])) !== first;
  }

  /** `count` reads of live word `index`, `gapMs` of emulated time apart, with the emulated time of each. */
  async sample(index: number, count: number, gapMs: number): Promise<Sample[]> {
    const out: Sample[] = [];
    for (let n = 0; n < count; n++) {
      out.push({ v: await this.live(index), t: await this.rt.clock.now() });
      await this.rt.ui.wait({ ms: gapMs, why: SAMPLING });
    }
    return out;
  }

  /**
   * Make the LFO run into one destination at a speed the fit can follow: locate the knobs (DEPTH last, so nothing is modulated while they are found), raise DEPTH above
   * its centre, choose the first of `DEST_CANDIDATES` whose live word varies, and adjust the speed until a window of 40 reads at 12 ms holds 6 to 14 crossings of its
   * middle. Prechecks, each with its own message: DEPTH went above its centre; the destination word moves ("no tick" when the tick's phase does not move, "wrong
   * destination" when the tick runs and other words of the row move or none does); the swing is at least `MIN_SWING` and does not clip.
   * Returns the index of the destination word.
   */
  async run(): Promise<number> {
    await this.open();
    for (const knob of ['DEST', 'SPD', 'WAVE', 'SPH', 'MODE', 'DEPTH'] as const) if (this.index[knob] === undefined) await this.locate(knob);
    await this.turnTo('DEPTH', DEPTH_WANT, 0x300);
    const depth = await this.word(this.index.DEPTH!);
    expect(depth, `DEPTH is above its centre ${DEPTH_ZERO.toString(16)} (the output is scaled by (depth - centre) * 2)`).toBeGreaterThan(DEPTH_ZERO + 0x800);
    let dest = -1;
    const others: string[] = [];
    for (const d of DEST_CANDIDATES) {
      if (dest >= 0) break;
      await this.turnTo('DEST', d << 8);
      const moving = await this.moving();
      if (moving.includes(d)) dest = d;
      else others.push(`DEST ${d}: ${moving.length === 0 ? 'no live word moves' : `live words ${moving.join(',')} move, not ${d}`}`);
    }
    if (dest < 0) {
      const ticks = await this.phaseMoves();
      throw new Error(
        ticks
          ? `wrong destination: the tick runs (the phase of LFO 1 moves) but no candidate destination word moves in the live row (${others.join('; ')}); the output is not where finding 143 says, or DEPTH/FADE give no swing`
          : `no tick: the phase of LFO 1 does not move and no live word varies (${others.join('; ')}); vector 191 is not running without the DSP, or SPD is the stopped value`,
      );
    }
    await this.tuneSpeed(dest);
    const swung = await this.sample(dest, 40, 12);
    expect(swing(swung), `the destination word ${dest} swings by at least ${MIN_SWING.toString(16)} peak to peak (DEPTH ${depth.toString(16)})`).toBeGreaterThan(MIN_SWING);
    const vs = swung.map((s) => s.v);
    expect(Math.min(...vs), `the destination word ${dest} does not clip at 0 (its base is too low for the depth)`).toBeGreaterThan(0);
    expect(Math.max(...vs), `the destination word ${dest} does not clip at 0x7f00 (its base is too high for the depth)`).toBeLessThan(0x7f00);
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

  /**
   * Stop the LFO (a speed of 0, the word 0x4000: the tick scales the rate by (speed - 0x4000)): the speed knob is moved one detent at a time over the middle of its range
   * until 4 reads of the destination 40 ms apart agree. Needs the destination of `run`, whose live word is static when the LFO stands still.
   */
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

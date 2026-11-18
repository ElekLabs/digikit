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
 * - the LFO does not write the Sound object: the tick adds its output to the destination word of the smoothed mirror that the DSP frame is built from (`lfo_live` of the device file), so the
 *   output is read there (`live`; `lfo_live`, `lfo_phase` and the row length `sound_words` are in `devices/*.toml`), and the knob words are read from the Sound object (`word`);
 * - SPD and DEPTH are bipolar around 0x4000: a speed of 0x4000 stops the LFO, and a depth of 0x4000 gives no output.
 */

export type Knob = 'SPD' | 'MULT' | 'DEST' | 'WAVE' | 'SPH' | 'MODE' | 'DEPTH';
const KNOB_ENCODER: Record<Knob, string> = {
  SPD: 'ENC A',
  MULT: 'ENC B',
  DEST: 'ENC D',
  WAVE: 'ENC E',
  SPH: 'ENC F',
  MODE: 'ENC G',
  DEPTH: 'ENC H',
};
/** The word of each knob of LFO 1 in the Sound array of track 1, the same on both devices (finding 143, "The knob words"); `locate` checks it. */
const KNOB_WORD: Record<Knob, number> = { SPD: 1, MULT: 2, DEST: 4, WAVE: 5, SPH: 6, MODE: 7, DEPTH: 8 };

/** The first array index that is no word of an LFO (1 to 24 are). The DEST word is `index << 8` (the tick reads its high byte as the array index, below the length of the row). */
const DEST_MIN = 25;
/** DEPTH is bipolar: the output amplitude is (depth - 0x4000) * 2 words times the shape. 0x5000 gives +-0x2000 around the base word. */
const DEPTH_WANT = 0x5000;
const DEPTH_ZERO = 0x4000;
/** The smallest peak-to-peak swing of the destination word that counts as an LFO output (the full swing at DEPTH_WANT is 0x4000 times the fade level). */
export const MIN_SWING = 0x400;

// ---- the rig -------------------------------------------------------------------------------------------------------------------------

const SAMPLING = 'emulated time between two reads of the LFO output (no observable state)';
const SMOOTH = 'the smoother glides a word that was just changed to its new value (no observable state)';
const GAP = 'the gap between encoder events (the stock encoder scales by event rate)';

/** Track 1, LFO 1, driven from the panel. Create it after the runtime is up; it reads the Sound words of track 1 from firmware memory. */
export class LfoRig {
  /** The Sound word of each knob, found by `locate` (or given by `known`). */
  readonly index: Partial<Record<Knob, number>> = {};
  /** The word of each knob before `locate` turned it. */
  private readonly initial: Partial<Record<Knob, number>> = {};
  private soundBase: number | undefined;
  private opened = false;

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

  /** Word `index` of the array the tick writes into (smoothed base + LFO output, the `lfo_live` row of the device file). */
  async live(index: number): Promise<number> {
    return halfword(this.session, deviceProfile(this.device).fx.lfo_live + 2 * index);
  }

  /** The array index the DEST word names now. */
  async destination(): Promise<number> {
    return (await this.word(this.index.DEST!)) >> 8;
  }

  /** The whole live row of track 1. */
  async liveWords(): Promise<number[]> {
    return Promise.all(Array.from({ length: deviceProfile(this.device).fx.sound_words }, (_, i) => this.live(i)));
  }

  /** The MOD page, LFO 1: one tap of MOD from the main screen. Another tap would go on to LFO 2 (the page counter shows 1/3, 2/3, 3/3), so a rig taps once and stays on the page. */
  async open(): Promise<void> {
    if (this.opened) return;
    this.opened = true;
    await this.rt.ui.tap('MOD', { until: { ms: 300, why: REDRAW } });
  }

  /** `count` events of `step` detents `gapMs` apart, then a settle for the firmware to store the last one. */
  async turn(knob: Knob, step: number, count = 1, gapMs = 2): Promise<void> {
    for (let i = 0; i < count; i++) await this.rt.ui.turn(encoderCode(KNOB_ENCODER[knob]), step, { until: { ms: gapMs, why: GAP } });
    await this.rt.ui.wait({ ms: 150, why: SETTLE });
  }

  /**
   * One burst of `knob` towards a word that is `error` away. The stock encoder filter (finding 03, "an encoder filter swallows the first counts after idle") returns nothing
   * for the first counts after a knob was idle, a single detent every 30 ms does nothing at all, and the first burst after idle jumps by an irregular amount (DN2 1.12: WAVE
   * 0x100 to 0x600 in one burst of 4 events of 8 detents). Once the knob is awake the word follows the burst: per burst a menu value (WAVE, MODE, DEST) moves by about
   * 1 unit of 0x100 for 2 events of 8 detents 30 ms apart and by 1 unit per 3 bursts for 8 events of 1 detent 2 ms apart; SPH moves by 0x400 and 0x200 for the same bursts. The
   * burst is sized by the distance and the caller reads the word back (`turnTo`).
   */
  private async step(knob: Knob, error: number): Promise<void> {
    const dir = error > 0 ? 1 : -1;
    const far = Math.abs(error);
    if (far > 0x2000) await this.turn(knob, dir * 8, 4, 30);
    else if (far > 0x600) await this.turn(knob, dir * 8, 2, 30);
    else await this.turn(knob, dir, 8, 2);
  }

  /** The Sound word of `knob`: turn it in bursts until exactly one word moves (the lead-in filter of the stock encoder swallows the first counts, finding 03). */
  async locate(knob: Knob): Promise<number> {
    const before = await this.words();
    for (let n = 0; n < 6; n++) {
      await this.turn(knob, 8, 4, 30);
      const after = await this.words();
      const moved = after.flatMap((v, i) => (v === before[i] ? [] : [i]));
      if (moved.length === 0) continue;
      expect(moved, `the ${knob} knob (${KNOB_ENCODER[knob]}) moves one Sound word`).toHaveLength(1);
      expect(moved[0], `${knob} is the Sound word the finding names`).toBe(KNOB_WORD[knob]);
      this.index[knob] = moved[0]!;
      this.initial[knob] = before[moved[0]!]!;
      return moved[0]!;
    }
    throw new Error(`${KNOB_ENCODER[knob]} (${knob}) moved no Sound word in six bursts: is the LFO page open?`);
  }

  /**
   * Move `knob` until its word is within `tol` of `want`, reading the word back after every burst; fails after `max` bursts, and when the word has not moved in 20 bursts in a
   * row (a limit short of `want`: the message says where it stopped).
   */
  async turnTo(knob: Knob, want: number, tol = 0, max = 150): Promise<number> {
    const index = this.index[knob] ?? (await this.locate(knob));
    let stuck = 0;
    let last = -1;
    for (let n = 0; n < max; n++) {
      const got = await this.word(index);
      if (Math.abs(got - want) <= tol) return got;
      stuck = got === last ? stuck + 1 : 0;
      if (stuck >= 20) throw new Error(`${knob} stops at ${got.toString(16)}, wanted ${want.toString(16)}`);
      await this.step(knob, want - got);
      last = got;
    }
    throw new Error(`${knob} did not reach ${want.toString(16)} in ${max} bursts (at ${(await this.word(index)).toString(16)})`);
  }

  /** WAVE to wave number `n`: the word is `n << 8`. Fails when the encoder stops short of it (the maximum of the parameter). */
  async setWave(n: number): Promise<void> {
    await this.turnTo('WAVE', n << 8);
  }

  /** The words of the live row that move by more than 0x100 over 6 reads 7 ms apart, not counting the knob words of this LFO (the smoother's rounding moves a word by a few units). */
  async moving(): Promise<number[]> {
    const knobWords = new Set(Object.values(this.index));
    const seen: number[][] = [];
    for (let n = 0; n < 6; n++) {
      seen.push(await this.liveWords());
      await this.rt.ui.wait({ ms: 7, why: SAMPLING });
    }
    return seen[0]!.flatMap((_, i) => {
      const v = seen.map((w) => w[i]!);
      return !knobWords.has(i) && Math.max(...v) - Math.min(...v) > 0x100 ? [i] : [];
    });
  }

  /** True when the phase accumulator of LFO 1 of track 1 moves between two reads 20 ms apart: the tick runs and the LFO is not stopped. */
  async phaseMoves(): Promise<boolean> {
    const address = deviceProfile(this.device).fx.lfo_phase;
    const first = await word(this.session, address);
    await this.rt.ui.wait({ ms: 20, why: SAMPLING });
    return (await word(this.session, address)) !== first;
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
   * Make the LFO run into one destination at a speed the fit can follow: locate the knobs (DEPTH last, so nothing is modulated while they are found; DEST in `chooseDest`), raise DEPTH above
   * its centre, choose the first destination of the list (`chooseDest`) whose live word follows the LFO, and set the rate (`tuneSpeed`) until a window of 40 reads at 12 ms holds 6 to 14 crossings of its
   * middle. Prechecks, each with its own message: DEPTH went above its centre; the destination word moves ("no tick" when the tick's phase does not move, "wrong
   * destination" when the tick runs and other words of the row move or none does); the swing is at least `MIN_SWING` and does not clip.
   * Returns the index of the destination word.
   */
  async run(): Promise<number> {
    await this.open();
    // DEST is located in `chooseDest`, after the others: its knob opens a list as a popup, and a turn of another knob closes it and cancels the choice
    for (const knob of ['SPD', 'MULT', 'WAVE', 'SPH', 'MODE', 'DEPTH'] as const) if (this.index[knob] === undefined) await this.locate(knob);
    // locating turned the knobs: WAVE, SPH and MODE go back to where they were (a MODE other than FRE stops the LFO after a cycle)
    for (const [knob, tol] of [['WAVE', 0], ['MODE', 0], ['SPH', 0x200]] as const) if (this.initial[knob] !== undefined) await this.turnTo(knob, this.initial[knob]!, tol);
    await this.turnTo('DEPTH', DEPTH_WANT, 0x300);
    const depth = await this.word(this.index.DEPTH!);
    expect(depth, `DEPTH is above its centre ${DEPTH_ZERO.toString(16)} (the output is scaled by (depth - centre) * 2)`).toBeGreaterThan(DEPTH_ZERO + 0x800);
    const dest = await this.chooseDest(depth);
    return dest;
  }

  /**
   * Scan the destination list of the page and take the first destination whose live word follows the LFO. The DEST knob steps through the list positions, and the word of a
   * position is the array index the loader's remap table gives it, so the words are not in order (DN2 1.12: three events of 64 detents gave +0x1d00, four of 32 gave
   * -0x1600): the word is read back after every burst and an index is tried when it is no word of an LFO and below the limit of the tick (`DEST_MIN` to the end of the row).
   * A candidate is refused with a reason when no live word moves, when the swing is too small or when it clips; the reasons go into the failure message.
   */
  private async chooseDest(depth: number): Promise<number> {
    const index = this.index.DEST ?? (await this.locate('DEST'));
    const others: string[] = [];
    const tried = new Set<number>();
    const visited = new Set<number>();
    let lastNew = 0;
    let known = 0;
    const destMax = deviceProfile(this.device).fx.sound_words - 1; // the tick takes a destination byte below the length of the row (DT2 < 0x47, DN2 < 0x65)
    for (let n = 0; n < 150 && n - lastNew < 30; n++) {
      const d = (await this.word(index)) >> 8;
      visited.add(d);
      if (d >= DEST_MIN && d <= destMax && !tried.has(d)) {
        tried.add(d);
        // the list is a popup: another knob cancels it and the word falls back to 0 (DN2 1.12), YES keeps the choice
        await this.rt.ui.tap('YES', { until: { ms: 400, why: REDRAW } });
        // the smoother glides the destination word from its old value to the base of the new one: read after it has settled
        await this.rt.ui.wait({ ms: 800, why: SMOOTH });
        const moving = await this.moving();
        if (!moving.includes(d)) {
          others.push(`DEST ${d}: ${moving.length === 0 ? 'no live word moves' : `live words ${moving.join(',')} move, not ${d}`}`);
        } else {
          try {
            await this.tuneSpeed(d);
          } catch (error) {
            others.push(`DEST ${d}: ${String(error)}`);
            await this.step('DEST', 0x800);
            continue;
          }
          const swung = await this.sample(d, 40, 12);
          const vs = swung.map((x) => x.v);
          const why =
            swing(swung) <= MIN_SWING
              ? `swing ${swing(swung).toString(16)} is under ${MIN_SWING.toString(16)}`
              : Math.min(...vs) <= 0
                ? 'clips at 0'
                : Math.max(...vs) >= 0x7f00
                  ? 'clips at 0x7f00'
                  : '';
          if (!why) return d;
          others.push(`DEST ${d}: moves but ${why} (DEPTH ${depth.toString(16)})`);
        }
      }
      if (visited.size > known) {
        known = visited.size;
        lastNew = n;
      }
      await this.step('DEST', 0x800);
    }
    const ticks = await this.phaseMoves();
    throw new Error(
      ticks
        ? `wrong destination: the tick runs (the phase of LFO 1 moves) but no destination of the list follows the LFO (${others.join('; ')}); the output is not where finding 143 says, or DEPTH/FADE give no swing`
        : `no tick: the phase of LFO 1 does not move and no live word varies (${others.join('; ')}); vector 191 is not running without the DSP, or SPD is the stopped value`,
    );
  }

  /**
   * Rate so that 40 reads at 12 ms hold 6 to 14 crossings (3 to 7 cycles, 6 to 13 reads per cycle). The rate is set with MULT, one unit of 0x100 at a time: SPD moves the rate
   * only a little (DN2 1.12, crossings at SPD 0x7000, 0x7800 and 0x7ffe: MULT 0x100 gives 1, 1 and 1; 0x300 gives 2, 2 and 2; 0x600 gives 12, 13 and 15; 0xa00 gives 29, 29 and 4,
   * which is the rate aliasing at the read interval).
   */
  async tuneSpeed(dest: number): Promise<void> {
    const mult = this.index.MULT ?? (await this.locate('MULT'));
    const seen: string[] = [];
    for (let n = 0; n < 12; n++) {
      const c = crossings(await this.sample(dest, 40, 12));
      const now = await this.word(mult);
      seen.push(`MULT ${now.toString(16)}: ${c}`);
      if (c >= 6 && c <= 14) return;
      await this.turnTo('MULT', Math.max(0x100, now + (c < 6 ? 0x100 : -0x100)), 0);
    }
    throw new Error(`the LFO rate could not be set to 3 to 7 cycles in 0.5 s of emulated time (crossings: ${seen.join(', ')})`);
  }

  /**
   * Stop the LFO (a speed of 0, the word 0x4000: the tick scales the rate by (speed - 0x4000)): the speed word is brought to within 4 of 0x4000 (`turnTo` reads it back) and 4 reads
   * of the destination 40 ms apart must agree. Needs the destination of `run`, whose live word is static when the LFO stands still.
   */
  async freeze(dest: number): Promise<void> {
    await this.turnTo('SPD', 0x4000, 4); // the fine steps move the word by 1 or 2: a speed within 4 of the middle is under 0.1% of the top rate, which stands still within 0.2 s
    const reads = await this.sample(dest, 4, 40);
    expect(
      reads.map((r) => r.v),
      'the destination word stands still at speed 0x4000',
    ).toEqual(reads.map(() => reads[0]!.v));
  }
}

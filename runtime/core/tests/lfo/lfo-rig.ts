import { encoder as encoderCode, pollUntil, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { REDRAW, SETTLE, word, type Rt } from '@digi/regression/runtime/fx-live.ts';
import { halfword } from '@digi/regression/runtime/fx-track-ui.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { captureRemoteOled } from '@digi/regression/runtime/remote.ts';
import { crossings, swing, type Sample } from './lfo-fit.ts';

export * from './lfo-fit.ts';

/**
 * Helpers of the LFO shape device tests (plan E6): the knobs of LFO 1 of track 1 set with the firmware's own MIDI CC path, the knob words read from the Sound object, the LFO output read from the
 * array the tick writes it into, and a phase-free fit of the output against a shape.
 *
 * What the tests assume of the firmware (finding 149, "Where the output lands" and "The knob words"; each is checked on the way and fails with a message, so a wrong
 * assumption shows at the first device run):
 * - the CC numbers of LFO 1 are SPD 102, MULT 103, FADE 104, DEST 105, WAVE 106, SPH 107, MODE 108, DEPTH 109 (the same on both devices); a CC value v is stored as `v << 8` in the knob word, and the
 *   DEST value is the array index itself (`set` reads the word back and fails when it differs);
 * - the knob words of LFO 1 are the halfwords 1 to 8 of the Sound array of track 1 in that order (`KNOB_WORD`);
 * - the LFO does not write the Sound object: the tick adds its output to the destination word of the smoothed mirror (`lfo_live` of the device file), which holds the bare base for part of every
 *   handler run; the handler then copies the Sound indices 25 on into the DSP transmit frame, which holds the final value until the next run, so the output is read there (`live`; `lfo_frame`,
 *   `lfo_phase` and the row length `sound_words` are in `devices/*.toml`), and the knob words are read from the Sound object (`word`);
 * - the LFO state belongs to a voice slot, not a track (`phase`, `trigger`): a device with voice rotation gives each note the next slot, a device with one voice per track keeps track 1 on slot 0;
 * - SPD and DEPTH are bipolar around 0x4000: a speed of 0x4000 stops the LFO, and a depth of 0x4000 gives no output.
 */

export type Knob = 'SPD' | 'MULT' | 'DEST' | 'WAVE' | 'SPH' | 'MODE' | 'DEPTH';
/** The MIDI CC number of each knob of LFO 1 (channel 1). */
const KNOB_CC: Record<Knob, number> = { SPD: 102, MULT: 103, DEST: 105, WAVE: 106, SPH: 107, MODE: 108, DEPTH: 109 };
const CC_FADE = 104;
/** The word of each knob of LFO 1 in the Sound array of track 1, the same on both devices (finding 149, "The knob words"). */
export const KNOB_WORD: Record<Knob, number> = { SPD: 1, MULT: 2, DEST: 4, WAVE: 5, SPH: 6, MODE: 7, DEPTH: 8 };

/** The first array index that is no word of an LFO (1 to 24 are). The DEST word is `index << 8` (the tick reads its high byte as the array index, below the length of the row). */
const DEST_MIN = 25;
/** A destination whose stored word is in this range does not clip with a swing of +-0x1000 around it (the live row is 0 to 0x7fff). */
const BASE_MIN = 0x2000;
const BASE_MAX = 0x5f00;
/** DEPTH is bipolar: the output amplitude is (depth - 0x4000) * 2 words times the shape. CC 80 gives 0x5000, which is +-0x2000 around the base word. */
const DEPTH_CC = 80;
/** The CC value of the stopped speed and of no fade: the word 0x4000 (bipolar, finding 149). */
const NEUTRAL_CC = 64;
/** MODE 0 is FRE (free running); the others are TRG, HLD, ONE, HLF. */
const MODE_FRE = 0;
/**
 * Rate so that 40 reads at 12 ms hold 6 to 14 crossings (3 to 7 cycles, 6 to 13 reads per cycle). Measured: SPD 0x7000 with MULT 0x600 gave 11 crossings; 0x500 is chosen for more reads per cycle. SPD moves the rate
 * only a little (crossings at SPD 0x7000, 0x7800 and 0x7ffe: MULT 0x100 gives 1, 1 and 1; 0x300 gives 2, 2 and 2; 0x600 gives 12, 13 and 15; 0xa00 gives 29, 29 and 4, which is the rate
 * aliasing at the read interval).
 */
const SPD_CC = 112;
const MULT_CC = 5;
/** The smallest peak-to-peak swing of the destination word that counts as an LFO output (the full swing at DEPTH_CC is 0x4000 times the fade level). */
export const MIN_SWING = 0x400;

// ---- the rig -------------------------------------------------------------------------------------------------------------------------

const SAMPLING = 'emulated time between two reads of the LFO output (no observable state)';
export const SMOOTH = 'the smoother glides a word that was just changed to its new value (no observable state)';
export const SMOOTH_MS = 800;
/** The most destination candidates `run` tries. */
const MAX_CANDIDATES = 4;
const TRIGGER = 'the note reaches the voice and the handler retriggers the LFO (no observable state)';
const GAP = 'the gap between encoder events (the stock encoder scales by event rate)';

/** Track 1, LFO 1, set with MIDI CC. Create it after the runtime is up; it reads the Sound words of track 1 from firmware memory. */
export class LfoRig {
  private soundBase: number | undefined;
  private opened = false;

  readonly rt: Rt;

  constructor(rt: Rt) {
    this.rt = rt;
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

  /**
   * Word `index` (25 to 25 + `lfo_frame_words` - 1) of voice slot `slot` in the DSP transmit frame: the final value (smoothed base + LFO output) of the last handler run, held until the next run.
   * The `lfo_live` row of the device file is not read: it holds the bare base for part of every run.
   */
  async live(index: number, slot = 0): Promise<number> {
    const { lfo_frame, lfo_frame_stride, lfo_frame_words } = deviceProfile(this.device).fx;
    if (index < DEST_MIN || index >= DEST_MIN + lfo_frame_words)
      throw new Error(`live word ${index} is not in the DSP frame (indices ${DEST_MIN} to ${DEST_MIN + lfo_frame_words - 1})`);
    return halfword(this.session, lfo_frame + lfo_frame_stride * slot + 2 * (index - DEST_MIN));
  }

  /** The array index the DEST word names now. */
  async destination(): Promise<number> {
    return (await this.word(KNOB_WORD.DEST)) >> 8;
  }

  /** The words of voice slot 0 in the DSP frame (indices 25 on). */
  async liveWords(): Promise<number[]> {
    return Promise.all(Array.from({ length: deviceProfile(this.device).fx.lfo_frame_words }, (_, i) => this.live(DEST_MIN + i)));
  }

  /** The MOD page, LFO 1: one tap of MOD from the main screen. Another tap would go on to LFO 2 (the page counter shows 1/3, 2/3, 3/3), so a rig taps once and stays on the page. */
  async open(): Promise<void> {
    if (this.opened) return;
    this.opened = true;
    await this.rt.ui.tap('MOD', { until: { ms: 300, why: REDRAW } });
  }

  /**
   * Forget that the MOD page is open: call it after anything that leaves the MOD page, such as a project save or load (the SETTINGS taps of `savePanelProject` and `loadPanelProject` leave it),
   * so that the next `open()` taps MOD again. It assumes that one MOD tap then opens MOD page 1.
   */
  closed(): void {
    this.opened = false;
  }

  /**
   * `count` events of `step` detents of the WAVE encoder (ENC E) `gapMs` apart, then (`settle`, default) a settle for the firmware to store the last one: no state shows the store of the last event
   * of a burst the stock filter may swallow. A caller that polls the Sound word itself passes `settle: false`. The page must be open.
   */
  async turnWave(step: number, count = 1, gapMs = 2, settle = true): Promise<void> {
    for (let i = 0; i < count; i++) await this.rt.ui.turn(encoderCode('ENC E'), step, { until: { ms: gapMs, why: GAP } });
    if (settle) await this.rt.ui.wait({ ms: 150, why: SETTLE });
  }

  /**
   * Turn the WAVE encoder until the Sound word is `n << 8`, then wait for its smoothed copy. The stock encoder filter swallows small bursts: one burst is 4 events of 8 detents 30 ms apart,
   * repeated (after each burst the word is polled) until the word is reached. Fails after 20 bursts with the last word. The page must be open.
   */
  async turnWaveTo(n: number): Promise<void> {
    const want = n << 8;
    let got = await this.word(KNOB_WORD.WAVE);
    let bursts = 0;
    while (got !== want) {
      if (bursts >= 20) {
        const png = await captureRemoteOled(this.rt.session, 'lfo-turn-fail');
        throw new Error(`the WAVE encoder did not reach wave ${n} in ${bursts} bursts (Sound word ${KNOB_WORD.WAVE} is ${got.toString(16)}, wanted ${want.toString(16)}; screen ${png})`);
      }
      await this.turnWave(got < want ? 8 : -8, 4, 30, false); // the poll of the Sound word below is the wait
      bursts++;
      let png = '';
      try {
        // the word is stable when two consecutive reads agree: the burst may still be stored
        let prev = -1;
        got = await pollUntil(
          this.rt.clock,
          async () => {
            const w = await this.word(KNOB_WORD.WAVE);
            const same = w === prev;
            prev = w;
            return same ? w : -1;
          },
          (w) => w !== -1,
          { withinMs: 1000, intervalMs: 20, message: () => `the WAVE word does not settle after burst ${bursts} toward wave ${n} (last read ${prev.toString(16)})` },
        );
      } catch (e) {
        png = await captureRemoteOled(this.rt.session, 'lfo-turn-fail');
        throw new Error(`${e instanceof Error ? e.message : String(e)} (screen ${png})`);
      }
    }
    let smooth = -1;
    await pollUntil(
      this.rt.clock,
      async () => (smooth = await this.smoothed(KNOB_WORD.WAVE)),
      (s) => s === want,
      { withinMs: 3000, intervalMs: 20, message: () => `the smoothed WAVE word is ${smooth.toString(16)}, wanted ${want.toString(16)}` },
    );
  }

  /** The smoother's copy of Sound word `index`, which the tick reads (word `index` of the `lfo_live` row). Stable for words 1 to 24, which are no LFO destination. */
  async smoothed(index: number): Promise<number> {
    return halfword(this.session, deviceProfile(this.device).fx.lfo_live + 2 * index);
  }

  /** Send CC `cc` on channel 1 and wait until the Sound word `word` and then its smoothed copy are `value << 8` (firmware state). */
  private async setCc(name: string, cc: number, word: number, value: number): Promise<void> {
    await sendCc(this.rt, 1, cc, value);
    const want = value << 8;
    // what the poll saw, for the failure message: the first and last Sound word read (no extra read on the pass path)
    let first: number | undefined;
    let seen: number | undefined;
    const read = async (): Promise<number> => {
      const got = await this.word(word);
      first ??= got;
      seen = got;
      return got;
    };
    try {
      await pollUntil(this.rt.clock, read, (got) => got === want, { withinMs: 1000, intervalMs: 20, message: `${name}: CC ${cc} = ${value}` });
    } catch (err) {
      const { fx, midi } = deviceProfile(this.device);
      const hex = (n: number | undefined) => (n === undefined ? 'none' : n.toString(16));
      const hits = (await this.session.status()).cf.mainloop_hits;
      const now = await this.rt.clock.now();
      throw new Error(
        `${name}: CC ${cc} = ${value} did not store the word ${want.toString(16)} in Sound word ${word}; ` +
          `Sound word first ${hex(first)}, last ${hex(seen)}; emulated time ${now} ms; mainloop_hits ${hits}; ` +
          `CC channel 1, track 1 factory channel ${midi?.track_channels[0] ?? 'unknown'}; ` +
          `sound base ${hex(this.soundBase)} (kit_ptr ${hex(fx.kit_ptr)}); ` +
          `not available: firmware MIDI receive count or input queue, active track (no address in devices/ or ports/fw); poll: ${String(err)}`,
      );
    }
    let last = -1;
    await pollUntil(
      this.rt.clock,
      async () => (last = await this.smoothed(word)),
      (got) => got === want,
      {
        withinMs: 3000,
        intervalMs: 20,
        message: () => `${name}: the smoother did not reach ${want.toString(16)} in word ${word} (last seen ${last.toString(16)})`,
      },
    );
  }

  /** Set `knob` to the CC value `value` (0 to 127): the word becomes `value << 8`. The tick reads the smoothed copy, so this returns once the tick sees the new value. */
  async set(knob: Knob, value: number): Promise<void> {
    await this.setCc(knob, KNOB_CC[knob], KNOB_WORD[knob], value);
  }

  /** WAVE to wave number `n`: the word is `n << 8`. With a shape loaded the CC reaches the shape numbers; the stock maximum is RAND. */
  async setWave(n: number): Promise<void> {
    await this.set('WAVE', n);
  }

  /** The 32-bit phase accumulator of LFO 1 of voice slot `slot`. */
  async phase(slot: number): Promise<number> {
    const { lfo_phase, lfo_phase_stride } = deviceProfile(this.device).fx;
    return word(this.session, lfo_phase + lfo_phase_stride * slot);
  }

  /** True when the phase accumulator of LFO 1 of voice slot 0 moves between two reads 20 ms apart: the tick runs and the LFO is not stopped. */
  async phaseMoves(): Promise<boolean> {
    const first = await this.phase(0);
    await this.rt.ui.wait({ ms: 20, why: SAMPLING });
    return (await this.phase(0)) !== first;
  }

  /**
   * Send a note (channel 1, note 60) and return the voice slot it took: the slot whose phase accumulator now equals `expected` (see `startPhase`). In MODE TRG the note retriggers the LFO of its slot (a device
   * with voice rotation gives each note the next slot; a device with one voice per track keeps slot 0). The other slots may belong to other tracks, whose LFOs run free, so their phases change too. Among the
   * slots at `expected`, the ones that changed are preferred; exactly one must remain. Throws with all phases before and after and `expected` otherwise.
   */
  async trigger(expected: number): Promise<number> {
    const { lfo_slots } = deviceProfile(this.device).fx;
    const phases = () => Promise.all(Array.from({ length: lfo_slots }, (_, s) => this.phase(s)));
    const before = await phases();
    await sendNote(this.rt, 1, 60, 100);
    await this.rt.ui.wait({ ms: 50, why: TRIGGER });
    await sendNoteOff(this.rt, 1, 60);
    await this.rt.ui.wait({ ms: 50, why: TRIGGER });
    const after = await phases();
    const atExpected = after.flatMap((p, s) => (p === expected ? [s] : []));
    const changed = atExpected.filter((s) => after[s] !== before[s]);
    const picked = changed.length > 0 ? changed : atExpected;
    if (picked.length !== 1) {
      const hex = (ps: number[]) => ps.map((p) => p.toString(16)).join(' ');
      throw new Error(
        `a note left ${picked.length} voice slots at the phase ${expected.toString(16)}, not 1 (before ${hex(before)}; after ${hex(after)})`,
      );
    }
    return picked[0]!;
  }

  /**
   * The phase a trigger sets for SPH `sph` (0 to 127): the tick sets phase = (smoothed SPH word << 16) * 0x5265c000 >> 31 (fractional MAC, finding 149), which is `sph * 0xA4CB80`.
   * The phase is 0 when WAVE is RAND.
   */
  static startPhase(sph: number): number {
    return sph * 0xa4cb80;
  }

  /** `count` reads of live word `index` of voice slot `slot`, `gapMs` of emulated time apart, with the emulated time of each. */
  async sample(index: number, count: number, gapMs: number, slot = 0): Promise<Sample[]> {
    const out: Sample[] = [];
    for (let n = 0; n < count; n++) {
      out.push({ v: await this.live(index, slot), t: await this.rt.clock.now() });
      await this.rt.ui.wait({ ms: gapMs, why: SAMPLING });
    }
    return out;
  }

  /**
   * Make the LFO run into one destination at a speed the fit can follow. The candidates are the first 4 array indexes from `DEST_MIN` whose stored word is in `BASE_MIN` to `BASE_MAX` (read once from
   * the Sound array), so a swing of +-0x1000 does not clip; the first one that passes the prechecks is taken. MODE (FRE), FADE (no fade), DEPTH, SPD and MULT are set with CC; WAVE stays as it is. Prechecks, each with its own message:
   * the destination word moves ("no tick" when the tick's phase does not move, "wrong destination" when the tick runs and other words of the row move or none does); the swing is at least
   * `MIN_SWING` and does not clip. Returns the index of the destination word.
   */
  async run(): Promise<number> {
    const stored = await this.words();
    const frameEnd = DEST_MIN + deviceProfile(this.device).fx.lfo_frame_words;
    const candidates: number[] = [];
    stored.forEach((w, i) => {
      // only the indices the handler copies into the DSP frame can be read there
      if (i >= DEST_MIN && i < frameEnd && w >= BASE_MIN && w <= BASE_MAX && candidates.length < MAX_CANDIDATES) candidates.push(i);
    });
    if (candidates.length === 0) throw new Error(`no Sound word from index ${DEST_MIN} is in ${BASE_MIN.toString(16)} to ${BASE_MAX.toString(16)}: ${stored.map((w) => w.toString(16)).join(' ')}`);
    await this.set('MODE', MODE_FRE);
    await this.setCc('FADE', CC_FADE, 3, NEUTRAL_CC);
    await this.set('DEPTH', DEPTH_CC);
    await this.set('SPD', SPD_CC);
    await this.set('MULT', MULT_CC);
    // a machine parameter may not be modulated on every machine, so the rig tries a few destinations
    const refused: string[] = [];
    for (const d of candidates) {
      await this.set('DEST', d);
      // the smoother glides the destination word to the base of the new destination: read after it has settled. The LFO swings the word, so no read of it shows the end of the glide: the wait stays.
      await this.rt.ui.wait({ ms: SMOOTH_MS, why: SMOOTH });
      const swung = await this.sample(d, 40, 12);
      const vs = swung.map((x) => x.v);
      const c = crossings(swung);
      console.info(`lfo-rig ${this.device}: DEST ${d} base ${stored[d]!.toString(16)} crossings ${c}`);
      let reason: string | undefined;
      if (swing(swung) === 0) {
        const ticks = await this.phaseMoves();
        reason = ticks
          ? `wrong destination: the tick runs (the phase of LFO 1 moves) but DEST ${d} does not follow the LFO; the output is not where finding 149 says, or DEPTH/FADE give no swing`
          : `no tick: the phase of LFO 1 does not move and DEST ${d} does not vary; vector 191 is not running without the DSP, or SPD is the stopped value`;
      } else if (swing(swung) < MIN_SWING) {
        reason = `the swing of DEST ${d} is below ${MIN_SWING.toString(16)}`;
      } else if (Math.min(...vs) <= 0) {
        reason = `DEST ${d} clips at 0`;
      } else if (Math.max(...vs) >= 0x7f00) {
        reason = `DEST ${d} clips at 0x7f00`;
      }
      if (reason === undefined) return d;
      console.info(`lfo-rig ${this.device}: DEST ${d} refused: ${reason}`);
      const sound = await Promise.all([1, 2, 3, 4, 5, 6, 7, 8].map((i) => this.word(i)));
      refused.push(
        `${reason} (min ${Math.min(...vs).toString(16)}, max ${Math.max(...vs).toString(16)}, first 12 reads ${vs
          .slice(0, 12)
          .map((v) => v.toString(16))
          .join(' ')}; Sound words 1..8 ${sound.map((v) => v.toString(16)).join(' ')})`,
      );
    }
    throw new Error(`no destination takes the LFO output after ${refused.length} tries:\n${refused.join('\n')}`);
  }

  /**
   * Stop the LFO (a speed of 0, the word 0x4000: the tick scales the rate by (speed - 0x4000)): SPD is set to the CC value 64 and 4 reads of the destination 40 ms apart must agree.
   * Needs the destination of `run`, whose live word is static when the LFO stands still.
   */
  async freeze(dest: number): Promise<void> {
    await this.set('SPD', NEUTRAL_CC);
    // `set` returned: the tick reads the stopped speed. The output follows once the phase of LFO 1 stands still (firmware state), then the next handler run copies the final word.
    await pollUntil(this.rt.clock, () => this.phaseMoves(), (moves) => !moves, {
      withinMs: 3 * SMOOTH_MS,
      intervalMs: 20,
      message: 'the phase of LFO 1 still moves after SPD was set to the stopped speed',
    });
    const reads = await this.sample(dest, 4, 40);
    expect(
      reads.map((r) => r.v),
      'the destination word stands still at speed 0x4000',
    ).toEqual(reads.map(() => reads[0]!.v));
  }
}

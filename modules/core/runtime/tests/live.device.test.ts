import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote, frames, pickSine, readFloatWav, word } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall, transferCommand } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Load and unload of a content module while the device runs (docs/plans/extension-model.md E5c): `LOAD` and `UNLOAD` run the same job as `RELOAD`.
 *
 * After every job the same checks of what the runtime keeps: RELOAD_STATE says finished with the expected result and `unloaded` flag; no new boot (the epoch); the `unloads`
 * and `reloads` counters of `DKRT_BOOT` moved by what the job did; the voice rows in use are those the modules' row masks name (no row of an unloaded module is left, and no
 * placeholder: a plain unload writes none); no `TABLE_BAD` in the log.
 *
 * Test 1 (both devices, code-only modules, a boot with the debug module alone):
 * - (e) `test/sine` is on the card but the machines service is not loaded (no module of the boot needed it): `LOAD` ends with `E_MISSING`, nothing is loaded and nothing is counted.
 * - (a) `test/c-demo` (needs no service) is on the card, not in the boot selection: `LOAD` loads it (`wb transfer load` the first time), LOADS says ACTIVE at a new selection index with its file name,
 *   MODULES lists it, the `reloads` counter went up. A second `LOAD` of the same path is `E_LOADED` at the queue.
 * - `UNLOAD` takes it down (`wb transfer unload`): not in MODULES, LOADS says UNLOADED at the same index, its file is still on the card. `LOAD` again gives the same index and ACTIVE.
 *
 * Test 2 (both devices, `test/sine`, a boot with the debug module and `test/sine`; the saved selection is then changed to the debug module alone, so the module is "outside the
 * selection" for the rest):
 * - (b) the machine plays a note with a pitch report (the pitch from the upward zero crossings of the left channel, and the peak); `UNLOAD` and `LOAD` of it; the same note plays
 *   at the same pitch and level again, the machine type keeps its number and its voice row.
 * - (c) `UNLOAD` with a note held over it ends with no fault; the module's sine is gone (its amplitude at its pitch after the unload is below 2% of the level before; the machine type is a gap row, which plays the first stock machine's sound, so the output is not silent), against a control capture of the same held note without an unload (at least 50%); AMP HOLD is NOTE, FX sends 0; the module is not in MODULES; the next note after
 *   the load is audible.
 * - (d) refusals: while the transport plays `UNLOAD` and `LOAD` are refused with `E_RELOAD_PLAYING`, the module is as it was and nothing is counted.
 * - the soak: unload and load of the machine, 5 cycles in the gate and 20 in the full tier (`DIGIKIT_TIER=full`), the capacity counts unchanged each cycle.
 *
 * Run only with the go of the owner (docs/plans/extension-model.md, "Device proof"). The sample-pool case is in the tests of the module that uses the pool (DT2).
 */
/** A module that needs no service and builds no menu (`init` logs one line; no builder call, no menu, no DSP part): the code-only case needs no service of the boot. */
const SHAPE_ID = 'test/c-demo';
/** The store paths (`modules/<kind>/<author>-<name>.dkm`, the device's packed files): set at the start of each test. */
let SHAPE_PATH = '';
let SINE_PATH = '';
const setPaths = (rt: TestRuntime): void => {
  const device = rt.session.options.device as 'dt2' | 'dn2';
  SHAPE_PATH = moduleStorePath(device, SHAPE_ID);
  SINE_PATH = moduleStorePath(device, 'test/sine');
};
const E_RELOAD_PLAYING = loadCode('E_RELOAD_PLAYING');
const E_MISSING = loadCode('E_MISSING');
const SOAK_CYCLES = process.env.DIGIKIT_TIER === 'full' ? 20 : 5;
/** Byte offsets in DkrtBootInfo (modules/core/runtime/abi.toml): the `unloads` counter, then the `reloads` counter. */
const BOOT_UNLOADS = 248;
const BOOT_RELOADS = 252;
const MIDI_CHANNEL = 1;
/** The AMP envelope CCs of the track (MIDI appendix of each manual: Digitakt II B.5, Digitone II C.5): HOLD (127 is NOTE: the hold lasts as long as the key is held) and DECAY. */
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
/** The track's FX send CCs (Digitakt II B.7, Digitone II C.7), set to 0 so that a capture holds the voice only. */
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
const NOTE = 57;
/** Blocks (1500 a second) a capture records. */
const CAPTURE_BLOCKS = 4000;

const moduleRows = async (rt: TestRuntime) => (await rt.dk.modules()).filter((m) => m.name !== DEBUG_MODULE);
const popcount = (n: number) => n.toString(2).replaceAll('0', '').length;
const nameOf = (modulePath: string) => modulePath.split('/').at(-1)!;

const epochOf = async (rt: TestRuntime): Promise<number> => {
  const e = (await rt.session.status()).epoch;
  if (typeof e !== 'number') throw new Error('daemon status must report a numeric epoch');
  return e;
};

interface Counters {
  unloads: number;
  reloads: number;
}

const counters = async (rt: TestRuntime, plan: RuntimePlan): Promise<Counters> => {
  const boot = planSymbol(plan, 'DKRT_BOOT');
  return { unloads: await word(rt.session, boot + BOOT_UNLOADS), reloads: await word(rt.session, boot + BOOT_RELOADS) };
};

/** The state of one module in LOADS, by file name. */
const loadsEntry = async (rt: TestRuntime, modulePath: string) => (await rt.dk.loadStates()).find((m) => m.name === nameOf(modulePath));

/** What every finished job leaves, see the header. `want` is the job's result and `unloaded` flag; `moved` the counter changes. */
async function expectJobLeftNothing(
  rt: TestRuntime,
  plan: RuntimePlan,
  why: string,
  epoch: number,
  before: Counters,
  want: { result: number; unloaded: boolean },
  moved: Counters,
): Promise<void> {
  const st = await rt.dk.reloadState();
  expect(st.phase, `${why}: RELOAD_STATE phase (2 = finished)`).toBe(2);
  expect({ result: st.result, unloaded: st.unloaded }, `${why}: RELOAD_STATE`).toEqual(want);
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  const after = await counters(rt, plan);
  expect({ unloads: after.unloads - before.unloads, reloads: after.reloads - before.reloads }, `${why}: the unloads and reloads counters`).toEqual(moved);
  const [cap, mods] = [await rt.dk.capacity(), await moduleRows(rt)];
  const named = mods.reduce((n, m) => n + popcount(m.rows), 0);
  expect(cap['VOICE_ROWS']?.used, `${why}: voice rows in use (the row masks of the modules name ${named}; a row of an unloaded module or a placeholder would be one more)`).toBe(named);
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
}

/** One job and its checks: `LOAD`, `UNLOAD`. */
async function liveChecked(
  rt: TestRuntime,
  plan: RuntimePlan,
  op: 'load' | 'unload',
  modulePath: string,
  why: string,
  epoch: number,
): Promise<void> {
  const before = await counters(rt, plan);
  const end = op === 'load' ? await rt.dk.loadAndWait(modulePath) : await rt.dk.unloadAndWait(modulePath);
  const loaded = op === 'load';
  expect(end, `${why}: the ${op} of ${modulePath}`).toEqual({ result: 0, unloaded: !loaded });
  // a load counts as a reload (the module set changed); an unload counts one teardown, and the job counts it as a change too
  await expectJobLeftNothing(rt, plan, why, epoch, before, end, { unloads: loaded ? 0 : 1, reloads: 1 });
}

// ---- test 1: code-only modules, E_MISSING, the CLI -------------------------------------------------------------------------------------

async function expectLiveCodeOnly(rt: TestRuntime): Promise<void> {
  setPaths(rt);
  const artifacts = liveArtifacts(rt.session, 'live-code');
  await cliInstall(rt.session, DEBUG_MODULE, artifacts, 300_000);
  // the files go on the card (with the services they need), the selection stays the debug module alone
  await transferCommand(rt.session, ['install', SHAPE_ID, 'test/sine'], path.join(artifacts, 'install-files.log'), 300_000);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT']);
  const epoch = await epochOf(rt);
  const start = await rt.dk.capacity();
  expect((await moduleRows(rt)).map((m) => m.name), 'the boot loaded the debug module alone').toEqual([]);

  // (e) a machine whose service is not loaded: the plan answers E_MISSING, and nothing changes
  const before = await counters(rt, plan);
  const missing = await rt.dk.loadAndWait(SINE_PATH);
  expect(missing, '(e) the machines service is not loaded: LOAD is refused before anything changed').toEqual({ result: E_MISSING, unloaded: false });
  expect(await counters(rt, plan), '(e) a refusal is not counted').toEqual(before);
  expect((await moduleRows(rt)).map((m) => m.name), '(e) the machine is not loaded').toEqual([]);
  expect(await loadsEntry(rt, SINE_PATH), '(e) a refused load leaves no entry in LOADS').toBeUndefined();

  // (a) a code-only module outside the boot selection, through the CLI the first time
  const rows = await rt.dk.loadStates();
  const events = await transferCommand(rt.session, ['load', SHAPE_PATH], path.join(artifacts, 'cli-load.log'), 120_000);
  expect(events.at(-1), '(a) wb transfer load: the done event lists the path').toMatchObject({ event: 'done', op: 'load', done: [SHAPE_PATH] });
  await expectJobLeftNothing(rt, plan, '(a) wb transfer load', epoch, before, { result: 0, unloaded: false }, { unloads: 0, reloads: 1 });
  const shape = await loadsEntry(rt, SHAPE_PATH);
  expect(shape?.state, '(a) LOADS says ACTIVE').toBe('ACTIVE');
  expect(shape?.index, '(a) the load took a new selection index').toBe(rows.length);
  expect((await moduleRows(rt)).map((m) => m.name), '(a) MODULES lists it').toEqual([SHAPE_ID]);
  await expect(rt.dk.loadAndWait(SHAPE_PATH), '(a) a second load of the same path').rejects.toMatchObject({ status: 'E_LOADED' });

  // the unload (CLI), then the load again: the same index
  const gone = await transferCommand(rt.session, ['unload', SHAPE_PATH], path.join(artifacts, 'cli-unload.log'), 120_000);
  expect(gone.at(-1), 'wb transfer unload').toMatchObject({ event: 'done', op: 'unload', done: [SHAPE_PATH] });
  expect((await moduleRows(rt)).map((m) => m.name), 'after the unload MODULES does not list it').toEqual([]);
  const unloaded = await loadsEntry(rt, SHAPE_PATH);
  expect({ state: unloaded?.state, index: unloaded?.index }, 'LOADS says UNLOADED at the same index').toEqual({ state: 'UNLOADED', index: shape?.index });
  expect((await rt.dk.list()).map((e) => e.path), 'the file is still on the card').toContain(SHAPE_PATH);
  await liveChecked(rt, plan, 'load', SHAPE_PATH, '(a) load again', epoch);
  expect(await loadsEntry(rt, SHAPE_PATH), 'the same index and ACTIVE again').toMatchObject({ state: 'ACTIVE', index: shape?.index });
  await liveChecked(rt, plan, 'unload', SHAPE_PATH, '(a) unload again', epoch);
  const end = await rt.dk.capacity();
  for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
    expect(end[kind]?.used, `${kind} in use is as it was before the loads`).toBe(start[kind]?.used);
  }
  record(artifacts, 'loads', await rt.dk.loadStates());
}

// ---- test 2: a machine --------------------------------------------------------------------------------------------------------------

/** Upward zero crossings of `xs` at the stride `step` (the samples are interleaved stereo): the frequency over the span between the first and the last. */
function crossingsHz(xs: Float32Array, step: number, rate: number): number {
  const at: number[] = [];
  for (let i = step; i < xs.length; i += step) if (xs[i - step]! < 0 && xs[i]! >= 0) at.push(i / step);
  if (at.length < 3) throw new Error(`fewer than two periods in ${xs.length / step} samples`);
  return (rate * (at.length - 1)) / (at.at(-1)! - at[0]!);
}

const peakOf = (xs: Float32Array, from: number, to: number): number => {
  if (from < 0 || to > xs.length || to <= from) throw new Error(`window ${from}..${to} is outside the PCM of ${xs.length} samples`);
  let p = 0;
  for (let i = from; i < to; i++) p = Math.max(p, Math.abs(xs[i]!));
  return p;
};

/** A note held for `hold` emulated ms with the PCM captured over `CAPTURE_BLOCKS` blocks; the pitch (Hz of the left channel over the second half of the note) and the peak. */
async function notePitch(rt: TestRuntime, artifacts: string, tag: string, hold = 600): Promise<{ hz: number; peak: number }> {
  const file = path.join(artifacts, `${tag}.wav`);
  const trigger = (await frames(rt.session)) + 300;
  await captureNote(rt.session, {
    file,
    trigger,
    tag,
    length: CAPTURE_BLOCKS,
    noPress: true,
    during: async () => {
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, hold);
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
    },
  });
  const pcm = readFloatWav(file);
  // stereo: every second sample is the left channel; the span after the attack and before the release
  const perBlock = pcm.length / CAPTURE_BLOCKS;
  const from = Math.floor(perBlock * 250);
  const to = Math.floor(perBlock * 700);
  const left = new Float32Array((to - from) / 2);
  for (let i = 0; i < left.length; i++) left[i] = pcm[from + 2 * i]!;
  const rate = 48_000;
  return { hz: crossingsHz(left, 1, rate), peak: peakOf(pcm, from, to) };
}

async function expectLiveMachine(rt: TestRuntime): Promise<void> {
  setPaths(rt);
  const artifacts = liveArtifacts(rt.session, 'live-machine');
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT']);
  const epoch = await epochOf(rt);
  const start = await rt.dk.capacity();
  // the next boot would load the debug module alone: the machine is outside the saved selection from here on
  await rt.dk.select([moduleStorePath(rt.session.options.device as 'dt2' | 'dn2', DEBUG_MODULE)]);
  const sine = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  await pickSine(rt);
  const device = rt.session.options.device as 'dt2' | 'dn2';
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].decay, 64);
  for (const cc of SEND_CC[device]) await sendCc(rt, MIDI_CHANNEL, cc, 0);

  // (b) the pitch report before and after an unload and a load; the module keeps its selection index through both
  const bootIndex = (await loadsEntry(rt, SINE_PATH))?.index;
  expect(bootIndex, '(b) the machine has a selection index from the boot').toBeGreaterThanOrEqual(0);
  const first = await notePitch(rt, artifacts, 'before');
  expect(first.peak, `(b) the machine is audible (peak ${first.peak.toFixed(4)})`).toBeGreaterThan(0.01);
  await liveChecked(rt, plan, 'unload', SINE_PATH, '(b) unload', epoch);
  expect((await moduleRows(rt)).map((m) => m.name), '(b) the machine is gone').toEqual([]);
  expect(await loadsEntry(rt, SINE_PATH), '(b) LOADS says UNLOADED').toMatchObject({ state: 'UNLOADED', index: bootIndex });
  await liveChecked(rt, plan, 'load', SINE_PATH, '(b) load', epoch);
  const again = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  expect(again.types, '(b) the machine type keeps its number (a track that names it plays it again)').toEqual(sine.types);
  expect(again.rows, '(b) the voice row is the one it had').toBe(sine.rows);
  expect(await loadsEntry(rt, SINE_PATH), '(b) LOADS says ACTIVE at the index it had').toMatchObject({ state: 'ACTIVE', index: bootIndex });
  const second = await notePitch(rt, artifacts, 'after');
  record(artifacts, 'pitch-report', { before: first, after: second });
  expect(Math.abs(second.hz / first.hz - 1), `(b) the pitch after the load (${second.hz.toFixed(2)} Hz) is the pitch before (${first.hz.toFixed(2)} Hz)`).toBeLessThan(0.01);
  expect(second.peak, `(b) the level after the load (${second.peak.toFixed(4)}) is the level before (${first.peak.toFixed(4)})`).toBeGreaterThan(first.peak * 0.5);

  // (c) an unload with a note held over it: no fault, and the module's sound stops. A plain unload leaves the machine type a gap row, which plays the first stock machine's
  // sound (the state after "untick and restart"), so the held key keeps sounding with that sound: the check is that the module's own sine is gone, not that the output is silent.
  // The sine is measured at its pitch (found in the level before the unload) over a window that begins after the unload ended: its amplitude there is below 2% of the
  // amplitude before. A control capture of the same note held without an unload keeps at least 50% of it over the same window, so the window itself holds the sine when
  // the module is loaded. AMP HOLD is NOTE and the FX sends are 0 (set above).
  await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
  const WINDOW = [1100, 1700]; // blocks after the trigger: the unload ends before 1000 on both devices (the check below), the key is held to past the end
  const leftOf = (pcm: Float32Array, a: number, b: number) => {
    const perBlock = pcm.length / CAPTURE_BLOCKS;
    const out = new Float32Array(Math.floor(((b - a) * perBlock) / 2));
    const from = Math.floor(a * perBlock);
    for (let i = 0; i < out.length; i++) out[i] = pcm[from + 2 * i]!;
    return out;
  };
  /** The amplitude of the sine of `hz` in `xs` (Goertzel, one bin). */
  const amp = (xs: Float32Array, hz: number): number => {
    let re = 0;
    let im = 0;
    for (let n = 0; n < xs.length; n++) {
      const w = (2 * Math.PI * hz * n) / 48_000;
      re += xs[n]! * Math.cos(w);
      im -= xs[n]! * Math.sin(w);
    }
    return (2 * Math.hypot(re, im)) / xs.length;
  };
  const controlFile = path.join(artifacts, 'held-control.wav');
  const controlTrigger = (await frames(rt.session)) + 300;
  await captureNote(rt.session, {
    file: controlFile,
    trigger: controlTrigger,
    tag: 'held note control',
    length: CAPTURE_BLOCKS,
    noPress: true,
    during: async () => {
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, 1400); // longer than the window ends (1700 blocks is 1130 ms)
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
    },
  });
  const heldFile = path.join(artifacts, 'held.wav');
  const mark = { start: 0, end: 0 };
  const heldTrigger = (await frames(rt.session)) + 300;
  await captureNote(rt.session, {
    file: heldFile,
    trigger: heldTrigger,
    tag: 'unload with a note held',
    length: CAPTURE_BLOCKS,
    noPress: true,
    during: async () => {
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, 400); // the note sounds before the unload starts
      mark.start = (await frames(rt.session)) - heldTrigger;
      await liveChecked(rt, plan, 'unload', SINE_PATH, '(c) unload with a note held', epoch);
      mark.end = (await frames(rt.session)) - heldTrigger;
      await waitClock(rt.clock, 800); // the key stays down after the unload
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
    },
  });
  await rt.session.expectNoFault('(c) unload with a note held');
  expect((await moduleRows(rt)).map((m) => m.name), '(c) the machine is gone').toEqual([]);
  expect(mark.start, '(c) the unload started after the note began').toBeGreaterThan(300);
  expect(mark.end + 50, `(c) the unload (${mark.start}..${mark.end}) ended before the window (${WINDOW[0]}..${WINDOW[1]} blocks)`).toBeLessThanOrEqual(WINDOW[0]!);
  const held = readFloatWav(heldFile);
  const control = readFloatWav(controlFile);
  const levelBefore = leftOf(held, mark.start - 200, mark.start);
  let hz = 220;
  for (let f = 200; f <= 240; f += 0.5) if (amp(levelBefore, f) > amp(levelBefore, hz)) hz = f;
  const ampBefore = amp(levelBefore, hz);
  const ampAfter = amp(leftOf(held, WINDOW[0]!, WINDOW[1]!), hz);
  const ampControl = amp(leftOf(control, WINDOW[0]!, WINDOW[1]!), hz);
  record(artifacts, 'held-unload', { start: mark.start, end: mark.end, hz, ampBefore, ampAfter, ampControl });
  expect(ampBefore, `(c) the module's sine is audible before the unload (${ampBefore.toFixed(4)} at ${hz} Hz)`).toBeGreaterThan(0.01);
  expect(ampControl, `(c) the control without an unload still has the sine in the window (${ampControl.toFixed(4)} against ${ampBefore.toFixed(4)} before)`).toBeGreaterThan(ampBefore * 0.5);
  expect(ampAfter, `(c) the module's sine is gone after the unload (${ampAfter.toFixed(5)} at ${hz} Hz against ${ampBefore.toFixed(4)} before)`).toBeLessThan(ampBefore * 0.02);
  await liveChecked(rt, plan, 'load', SINE_PATH, '(c) load after the held note', epoch);
  const third = await notePitch(rt, artifacts, 'after-held');
  expect(third.peak, `(c) a new note after the load is audible (peak ${third.peak.toFixed(4)})`).toBeGreaterThan(first.peak * 0.5);

  // (d) the transport plays: UNLOAD and LOAD are refused with the old state, nothing counted
  await rt.ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
  const before = await counters(rt, plan);
  expect(await rt.dk.unloadAndWait(SINE_PATH), '(d) UNLOAD while the transport plays').toEqual({ result: E_RELOAD_PLAYING, unloaded: false });
  expect((await moduleRows(rt)).find((m) => m.name === 'test/sine')?.types, '(d) the module is live').toEqual(sine.types);
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the transport stops' } });
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the second STOP resets the transport' } });
  await liveChecked(rt, plan, 'unload', SINE_PATH, '(d) unload after STOP', epoch);
  await rt.ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
  expect(await rt.dk.loadAndWait(SINE_PATH), '(d) LOAD while the transport plays').toEqual({ result: E_RELOAD_PLAYING, unloaded: false });
  expect((await moduleRows(rt)).map((m) => m.name), '(d) nothing was loaded').toEqual([]);
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the transport stops' } });
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the second STOP resets the transport' } });
  expect(await counters(rt, plan), '(d) the refusals are not counted').toEqual({ unloads: before.unloads + 1, reloads: before.reloads + 1 });
  await liveChecked(rt, plan, 'load', SINE_PATH, '(d) load after STOP', epoch);

  // the soak
  const base = await rt.dk.capacity();
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    await liveChecked(rt, plan, 'unload', SINE_PATH, `soak cycle ${cycle}`, epoch);
    await liveChecked(rt, plan, 'load', SINE_PATH, `soak cycle ${cycle}`, epoch);
    const cap = await rt.dk.capacity();
    const m = (await moduleRows(rt)).find((x) => x.name === 'test/sine')!;
    expect(m.types, `cycle ${cycle}: the machine type moved`).toEqual(sine.types);
    expect(m.rows, `cycle ${cycle}: the voice row moved`).toBe(sine.rows);
    expect(m.faults, `cycle ${cycle}: the guard counted a fault`).toBe(0);
    for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
      expect(cap[kind]?.used, `cycle ${cycle}: ${kind} in use grew`).toBe(base[kind]?.used);
    }
  }
  const end = await rt.dk.capacity();
  for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
    expect(end[kind]?.used, `${kind} in use at the end is as at the start`).toBe(start[kind]?.used);
  }
  record(artifacts, 'soak', { cycles: SOAK_CYCLES, capacity: end });
}

runtimeTests('LOAD and UNLOAD of code-only modules: E_MISSING without the service, a load outside the boot selection, the same index again', expectLiveCodeOnly, { dsp: 'sharc' });
runtimeTests('LOAD and UNLOAD of a machine: the pitch before and after, an unload with a note held, the transport refusal and the soak', expectLiveMachine, { dsp: 'sharc' });

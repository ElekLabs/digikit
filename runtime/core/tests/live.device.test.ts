import path from 'node:path';
import { waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote } from '@digi/regression/runtime/note-capture.ts';
import { frames } from '@digi/regression/runtime/dsp-state.ts';
import { pickSine } from '@digi/regression/runtime/panel-waits.ts';
import { readFloatWav } from '@digi/regression/runtime/pcm-files.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/debug-module.ts';
import { moduleStorePath } from '@digi/regression/runtime/module-artifact.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/runtime-tests.ts';
import { cliInstall, transferCommand } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Load and unload of a content module while the device runs (docs/plans/extension-model.md E5c): `LOAD` and `UNLOAD` run the same job as `RELOAD`.
 *
 * After every job the checks of what only the firmware shows: the job ends with the expected result and `unloaded` flag; no new boot (the epoch); the voice rows in use are those
 * the modules' row masks name (an unloaded module leaves the MISSING placeholder of its number, E5d, which CAPACITY does not count: the rows in use are the rows the masks name). The runtime's table checks found no damage in the real firmware tables (no `TABLE_BAD` in the log). What the runtime keeps on the host is not read here: the
 * job's phases, refusals (`E_MISSING`, `E_LOADED`, `E_RELOAD_PLAYING`) and counters are unit and assembly tests (`reload.rs`, `loader`), a service's rows across an unload and a load
 * are the service contract suite (`dkrt_test_support::contract`); the one real transport refusal is in `reload.device.test.ts`.
 *
 * Test 1 (both devices, code-only modules, a boot with the debug module alone):
 * - (a) `test/c-demo` (needs no service) is on the card, not in the boot selection: `LOAD` loads it (`wb transfer load` the first time), LOADS says ACTIVE at a new selection index with its file name,
 *   MODULES lists it.
 * - `UNLOAD` takes it down (`wb transfer unload`): not in MODULES, LOADS says UNLOADED at the same index, its file is still on the card. `LOAD` again gives the same index and ACTIVE.
 *
 * Test 2 (both devices, `test/sine`, a boot with the debug module and `test/sine`; the saved selection is then changed to the debug module alone, so the module is "outside the
 * selection" for the rest):
 * - (b) the machine plays a note with a pitch report (the pitch from the upward zero crossings of the left channel, and the peak); `UNLOAD` and `LOAD` of it; the same note plays
 *   at the same pitch and level again, the machine type keeps its number and its voice row.
 * - (c) `UNLOAD` with a note held over it ends with no fault; the module's sine is gone (its amplitude at its pitch after the unload is below 2% of the level before; the machine type reads MISSING and its placeholder voice row renders silence: the peak over the window after the unload is below 1e-4), against a control capture of the same held note without an unload (at least 50%); AMP HOLD is NOTE, FX sends 0; the module is not in MODULES; the next note after
 *   the load is audible.
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

/** The state of one module in LOADS, by file name. */
const loadsEntry = async (rt: TestRuntime, modulePath: string) => (await rt.dk.loadStates()).find((m) => m.name === nameOf(modulePath));

/** What every finished job leaves, see the header. `want` is the job's result and `unloaded` flag. */
async function expectJobLeftNothing(rt: TestRuntime, why: string, epoch: number, want: { result: number; unloaded: boolean }): Promise<void> {
  const st = await rt.dk.reloadState();
  expect({ result: st.result, unloaded: st.unloaded }, `${why}: RELOAD_STATE`).toEqual(want);
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  const [cap, mods] = [await rt.dk.capacity(), await moduleRows(rt)];
  const named = mods.reduce((n, m) => n + popcount(m.rows), 0);
  // a MISSING placeholder row of an absent number is no row in use (it gives way to a real load, E5d): CAPACITY counts the rows the modules own
  expect(cap['VOICE_ROWS']?.used, `${why}: voice rows in use (the row masks of the modules name ${named}; a placeholder is not counted)`).toBe(named);
  // the runtime's table checks ran over the real firmware tables (host tests run them over fakes only)
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
}

/** One job and its checks: `LOAD`, `UNLOAD`. */
async function liveChecked(rt: TestRuntime, op: 'load' | 'unload', modulePath: string, why: string, epoch: number): Promise<void> {
  const end = op === 'load' ? await rt.dk.loadAndWait(modulePath) : await rt.dk.unloadAndWait(modulePath);
  const loaded = op === 'load';
  expect(end, `${why}: the ${op} of ${modulePath}`).toEqual({ result: 0, unloaded: !loaded });
  await expectJobLeftNothing(rt, why, epoch, end);
}

// ---- test 1: code-only modules, the CLI -------------------------------------------------------------------------------------

async function expectLiveCodeOnly(rt: TestRuntime): Promise<void> {
  setPaths(rt);
  const artifacts = liveArtifacts(rt.session, 'live-code');
  await cliInstall(rt.session, DEBUG_MODULE, artifacts, 300_000);
  // the files go on the card (with the services they need), the selection stays the debug module alone
  await transferCommand(rt.session, ['install', SHAPE_ID], path.join(artifacts, 'install-files.log'), 300_000);
  const epoch = await epochOf(rt);
  const start = await rt.dk.capacity();
  expect((await moduleRows(rt)).map((m) => m.name), 'the boot loaded the debug module alone').toEqual([]);

  // (a) a code-only module outside the boot selection, through the CLI the first time
  const rows = await rt.dk.loadStates();
  const events = await transferCommand(rt.session, ['load', SHAPE_PATH], path.join(artifacts, 'cli-load.log'), 120_000);
  expect(events.at(-1), '(a) wb transfer load: the done event lists the path').toMatchObject({ event: 'done', op: 'load', done: [SHAPE_PATH] });
  await expectJobLeftNothing(rt, '(a) wb transfer load', epoch, { result: 0, unloaded: false });
  const shape = await loadsEntry(rt, SHAPE_PATH);
  expect(shape?.state, '(a) LOADS says ACTIVE').toBe('ACTIVE');
  expect(shape?.index, '(a) the load took a new selection index').toBe(rows.length);
  expect((await moduleRows(rt)).map((m) => m.name), '(a) MODULES lists it').toEqual([SHAPE_ID]);

  // the unload (CLI), then the load again: the same index
  const gone = await transferCommand(rt.session, ['unload', SHAPE_PATH], path.join(artifacts, 'cli-unload.log'), 120_000);
  expect(gone.at(-1), 'wb transfer unload').toMatchObject({ event: 'done', op: 'unload', done: [SHAPE_PATH] });
  expect((await moduleRows(rt)).map((m) => m.name), 'after the unload MODULES does not list it').toEqual([]);
  const unloaded = await loadsEntry(rt, SHAPE_PATH);
  expect({ state: unloaded?.state, index: unloaded?.index }, 'LOADS says UNLOADED at the same index').toEqual({ state: 'UNLOADED', index: shape?.index });
  expect((await rt.dk.list()).map((e) => e.path), 'the file is still on the card').toContain(SHAPE_PATH);
  await liveChecked(rt, 'load', SHAPE_PATH, '(a) load again', epoch);
  expect(await loadsEntry(rt, SHAPE_PATH), 'the same index and ACTIVE again').toMatchObject({ state: 'ACTIVE', index: shape?.index });
  await liveChecked(rt, 'unload', SHAPE_PATH, '(a) unload again', epoch);
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
  await liveChecked(rt, 'unload', SINE_PATH, '(b) unload', epoch);
  expect((await moduleRows(rt)).map((m) => m.name), '(b) the machine is gone').toEqual([]);
  expect(await loadsEntry(rt, SINE_PATH), '(b) LOADS says UNLOADED').toMatchObject({ state: 'UNLOADED', index: bootIndex });
  await liveChecked(rt, 'load', SINE_PATH, '(b) load', epoch);
  const again = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  expect(again.types, '(b) the machine type keeps its number (a track that names it plays it again)').toEqual(sine.types);
  expect(again.rows, '(b) the voice row is the one it had').toBe(sine.rows);
  expect(await loadsEntry(rt, SINE_PATH), '(b) LOADS says ACTIVE at the index it had').toMatchObject({ state: 'ACTIVE', index: bootIndex });
  const second = await notePitch(rt, artifacts, 'after');
  record(artifacts, 'pitch-report', { before: first, after: second });
  expect(Math.abs(second.hz / first.hz - 1), `(b) the pitch after the load (${second.hz.toFixed(2)} Hz) is the pitch before (${first.hz.toFixed(2)} Hz)`).toBeLessThan(0.01);
  expect(second.peak, `(b) the level after the load (${second.peak.toFixed(4)}) is the level before (${first.peak.toFixed(4)})`).toBeGreaterThan(first.peak * 0.5);

  // (c) an unload with a note held over it: no fault, and the module's sound stops. A plain unload leaves the machine type MISSING with a parked voice row (E5d), so the held key
  // renders silence. The sine is measured at its pitch (found in the level before the unload) over a window that begins after the unload ended: its amplitude there is below
  // 2% of the amplitude before, and the peak of the window is below 1e-4 (digital silence, no stock machine's sound). A control capture of the same note held without an
  // unload keeps at least 50% of it over the same window, so the window itself holds the sine when the module is loaded. AMP HOLD is NOTE and the FX sends are 0 (set above).
  await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
  // blocks after the trigger: the window starts 50 blocks after the unload ended (measured, not assumed: its length depends on the firmware's load) and the key is held to past its end
  const WINDOW = [0, 0];
  const WINDOW_BLOCKS = 600;
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
      await liveChecked(rt, 'unload', SINE_PATH, '(c) unload with a note held', epoch);
      mark.end = (await frames(rt.session)) - heldTrigger;
      await waitClock(rt.clock, 800); // the key stays down after the unload
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
    },
  });
  await rt.session.expectNoFault('(c) unload with a note held');
  expect((await moduleRows(rt)).map((m) => m.name), '(c) the machine is gone').toEqual([]);
  expect(mark.start, '(c) the unload started after the note began').toBeGreaterThan(300);
  WINDOW[0] = mark.end + 50;
  WINDOW[1] = WINDOW[0] + WINDOW_BLOCKS;
  expect(WINDOW[1], `(c) the window (${WINDOW[0]}..${WINDOW[1]} blocks) lies inside the capture`).toBeLessThan(CAPTURE_BLOCKS);
  await liveChecked(rt, 'load', SINE_PATH, '(c) load after the held note', epoch);
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
      await waitClock(rt.clock, Math.ceil(WINDOW[1]! / 1.5) + 100); // past the window's end (1500 blocks a second)
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
    },
  });
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
  const peakAfter = peakOf(held, Math.floor(WINDOW[0]! * (held.length / CAPTURE_BLOCKS)), Math.floor(WINDOW[1]! * (held.length / CAPTURE_BLOCKS)));
  expect(peakAfter, `(c) the held key renders silence after the unload (peak ${peakAfter.toExponential(2)})`).toBeLessThan(1e-4);
  expect(ampAfter, `(c) the module's sine is gone after the unload (${ampAfter.toFixed(5)} at ${hz} Hz against ${ampBefore.toFixed(4)} before)`).toBeLessThan(ampBefore * 0.02);
  const third = await notePitch(rt, artifacts, 'after-held');
  expect(third.peak, `(c) a new note after the load is audible (peak ${third.peak.toFixed(4)})`).toBeGreaterThan(first.peak * 0.5);

  const end = await rt.dk.capacity();
  for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
    expect(end[kind]?.used, `${kind} in use at the end is as at the start`).toBe(start[kind]?.used);
  }
  record(artifacts, 'capacity', { capacity: end });
}

runtimeTests('LOAD and UNLOAD of code-only modules: a load outside the boot selection, the same index again', expectLiveCodeOnly, { dsp: 'sharc' });
runtimeTests('LOAD and UNLOAD of a machine: the pitch before and after, an unload with a note held', expectLiveMachine, { dsp: 'sharc' });

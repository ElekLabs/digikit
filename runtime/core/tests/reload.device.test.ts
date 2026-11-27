import fs from 'node:fs';
import path from 'node:path';
import { pollUntil, waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote } from '@digi/regression/runtime/note-capture.ts';
import { frames, runFrames } from '@digi/regression/runtime/dsp-state.ts';
import { pickSine } from '@digi/regression/runtime/panel-waits.ts';
import { readFloatWav } from '@digi/regression/runtime/pcm-files.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/debug-module.ts';
import { moduleStorePath } from '@digi/regression/runtime/module-artifact.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { packModuleVariant, SINE_OCTAVE_UP } from '@digi/regression/runtime/module-variant.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/runtime-tests.ts';
import { cardRig } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { FIRMWARE, describeOn } from '@digi/regression/runtime/devices.ts';
import { buildPanelProject, dynType, loadPanelProject, savePanelProject, trackType } from '@digi/regression/runtime/project-kit.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Reload of a content module (docs/plans/extension-model.md E5): `RELOAD` swaps the code, DSP and keys of a loaded module without a boot.
 *
 * Every reload below is followed by the checks of what only the firmware shows: the reload answers result 0 with the old module torn down; no new boot (the epoch); no row or
 * placeholder is left over (the voice rows in use are those the modules' row masks name: a placeholder row would count in use and be in no mask); and the machine selection view
 * the hook remembered is still there (the `machine_view` entry of core/ui names the variable and the builder, and the variable is not 0: the rebuild of the list at the end of a reload
 * has a view to rebuild). The rebuilt list itself is not read. What the runtime keeps on the host is not read here: the job's steps, state and counters are unit tests
 * (`reload.rs`, `loader`), a service's rows and numbers across a reload are the service contract suite (`dkrt_test_support::contract`, case 12b).
 *
 * - (a) a code-only module (`test/lfo-shape`) reloads.
 * - (b) the machine `test/sine`, with a note held by MIDI over the reload and the PCM captured: the level before the reload is audible, at least 3 windows in a row inside the
 *   swap (queued to the new note; the AMP hold is set to NOTE by CC so a held note does not fade by itself) are below 1% of it (the placeholder row, not a stock sound), a control capture of the same note and span without a reload stays above that threshold over the whole
 *   span (so decay alone cannot make the quiet run), and a new note after the swap is audible again. The peaks per window are in the assertion messages and `pcm-windows.json`.
 *   The machine type keeps its number and its voice row.
 * - (c) the reload is refused while the transport plays (`E_RELOAD_PLAYING`, the old module is live and the counter did not move) and goes through after STOP. This is the one
 *   proof that the firmware's transport state reaches the refusal; the order and wording of the refusals are unit tests.
 * - project (DT2): a project that uses `test/sine` on track 1 (two parameter values, a trig with a p-lock and a sound lock, built on the panel) is saved, the machine is reloaded, the
 *   project is loaded: track 1 holds the machine type again and a note plays the same sound (pitch within 1%, peak within 5%). The stock project loader that resolves the type is the
 *   firmware's; the type number the reload keeps is (b) above.
 * - the pitch soak (docs/plans/extension-model.md E5b item 8): two builds of `test/sine` with the same module id and keys that differ in one constant (`packModuleVariant`: the DSP
 *   phase increment, one octave), put on the card alternately and reloaded with a note held, 5 cycles in the gate and 20 in the full tier (`DIGIKIT_TIER=full`). Each cycle: the
 *   pitch of a new note after the swap is that of the build just loaded (220 Hz or 440 Hz, within 2%), so a reload that kept the old DSP image would fail it; the machine type, its
 *   voice row and the guard's fault count are as they were.
 *
 * Run only with the go of the owner (docs/plans/extension-model.md, "Device proof").
 */
/** The store paths (`modules/<kind>/<author>-<name>.dkm`), set at the start of each test. */
let MODULE_PATH = '';
let SHAPE_PATH = '';
const setPaths = (rt: TestRuntime): void => {
  const device = rt.session.options.device as 'dt2' | 'dn2';
  MODULE_PATH = moduleStorePath(device, 'test/sine');
  SHAPE_PATH = moduleStorePath(device, 'test/lfo-shape');
};
const E_RELOAD_PLAYING = loadCode('E_RELOAD_PLAYING');
const SOAK_CYCLES = process.env.DIGIKIT_TIER === 'full' ? 20 : 5;
/** Byte offsets in DkrtBootInfo (runtime/core/abi.toml) and in the `machine_view` entry of core/ui (word 0: the view variable, word 1: the builder). */
const VIEW_INFO_VAR = 0;
const VIEW_INFO_BUILD = 4;
const MIDI_CHANNEL = 1;
/**
 * The AMP envelope CCs of the track (MIDI appendix of each manual: Digitakt II B.5, Digitone II C.5): HOLD and DECAY. HOLD = 127 is NOTE: the hold phase lasts as long as the key
 * is held (the default hold ends by itself and the note faded out in the middle of the measured span), the decay then only shapes the release after the note off.
 */
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
/**
 * The track's FX send CCs (Digitakt II B.7, Digitone II C.7): delay, reverb, chorus. They are set to 0 so that the capture holds the voice only: the default Digitone II sound sends
 * to the effects, and their tail after the voice is cut (a decorrelated 220 Hz at about -30 dB, left and right unlike) is no leak of the placeholder.
 */
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
const NOTE = 57;

const moduleRows = async (rt: TestRuntime) => (await rt.dk.modules()).filter((m) => m.name !== DEBUG_MODULE);
const popcount = (n: number) => n.toString(2).replaceAll('0', '').length;

const epochOf = async (rt: TestRuntime): Promise<number> => {
  const e = (await rt.session.status()).epoch;
  if (typeof e !== 'number') throw new Error('daemon status must report a numeric epoch');
  return e;
};

/** What every reload leaves: see the header. */
async function expectReloadLeftNothing(rt: TestRuntime, plan: RuntimePlan, why: string, epoch: number): Promise<void> {
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  const [cap, mods] = [await rt.dk.capacity(), await moduleRows(rt)];
  const named = mods.reduce((n, m) => n + popcount(m.rows), 0);
  expect(cap['VOICE_ROWS']?.used, `${why}: voice rows in use (the row masks of the modules name ${named}; a placeholder row would be one more)`).toBe(named);
  // the runtime's table checks ran over the real firmware tables (host tests run them over fakes only)
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
  const info = planSymbol(plan, 'ui_machine_view_info');
  const viewVar = await word(rt.session, info + VIEW_INFO_VAR);
  expect(viewVar, `${why}: the machine_view entry of core/ui names the variable that holds the machine selection view`).not.toBe(0);
  expect(await word(rt.session, info + VIEW_INFO_BUILD), `${why}: the machine_view entry names the list builder`).not.toBe(0);
  expect(await word(rt.session, viewVar), `${why}: the hook remembered the machine selection view`).not.toBe(0);
}

/** One reload of `modulePath`, then the checks of `expectReloadLeftNothing`. */
async function reloadChecked(rt: TestRuntime, plan: RuntimePlan, modulePath: string, why: string, epoch: number): Promise<void> {
  const end = await rt.dk.reloadAndWait(modulePath);
  expect(end, `${why}: the reload of ${modulePath}`).toEqual({ result: 0, unloaded: true });
  await expectReloadLeftNothing(rt, plan, why, epoch);
}

/** Blocks (1500 a second) each capture records: the note, the reload (a few hundred blocks) and the note after it fit with room to spare; `expect` below checks it. */
const CAPTURE_BLOCKS = 8000;
/** Blocks after the swap that the level of the new note is read over. */
const POST_BLOCKS = 300;

/** The largest magnitude of `xs[from..to)`. A window outside the recording is an error, not silence: an empty window reads 0 and would pass for a quiet one. */
const peakOf = (xs: Float32Array, from: number, to: number): number => {
  if (from < 0 || to > xs.length || to <= from) throw new Error(`window ${from}..${to} is outside the PCM of ${xs.length} samples`);
  let p = 0;
  for (let i = from; i < to; i++) p = Math.max(p, Math.abs(xs[i]!));
  return p;
};

async function expectReloadLive(rt: TestRuntime): Promise<void> {
  setPaths(rt);
  const artifacts = liveArtifacts(rt.session, 'reload-live');
  await cliInstall(rt.session, 'test/lfo-shape', artifacts, 300_000, [DEBUG_MODULE, 'test/sine']);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT', 'ui_machine_view_info']);
  const epoch = await epochOf(rt);
  const before = await moduleRows(rt);
  record(artifacts, 'modules-before', before);

  // (a) a code-only module
  await reloadChecked(rt, plan, SHAPE_PATH, '(a) code-only module', epoch);
  expect((await moduleRows(rt)).map((m) => m.name).sort(), '(a) the modules are the same').toEqual(before.map((m) => m.name).sort());

  // (b) the machine with DSP, a note held over the reload, PCM captured
  const sine = before.find((m) => m.name === 'test/sine')!;
  await pickSine(rt);
  const amp = AMP_CC[rt.session.options.device as 'dt2' | 'dn2'];
  await sendCc(rt, MIDI_CHANNEL, amp.hold, 127); // the note sounds as long as the key is held: the same in the capture with the reload and in the control
  await sendCc(rt, MIDI_CHANNEL, amp.decay, 64);
  for (const cc of SEND_CC[rt.session.options.device as 'dt2' | 'dn2']) await sendCc(rt, MIDI_CHANNEL, cc, 0); // the voice only: no effect tail in the swap
  const length = CAPTURE_BLOCKS;
  const WINDOWS = 32;
  /** Peaks of `WINDOWS` equal windows of blocks `a..b` of a recording of `length` blocks; a window past the recording throws. */
  const windowsOf = (pcm: Float32Array, a: number, b: number): number[] => {
    const perBlock = pcm.length / length;
    const step = Math.floor((b - a) / WINDOWS);
    if (step < 1) throw new Error(`the span ${a}..${b} is too short for ${WINDOWS} windows`);
    const peaks: number[] = [];
    for (let f = a; f + step <= b; f += step) peaks.push(peakOf(pcm, Math.floor(f * perBlock), Math.ceil((f + step) * perBlock)));
    return peaks;
  };
  const before0 = await frames(rt.session);
  const trigger = before0 + 300;
  const file = path.join(artifacts, 'swap.wav');
  const mark: { start: number; end: number } = { start: 0, end: 0 };
  await captureNote(rt.session, {
    file,
    trigger,
    tag: 'reload swap',
    length,
    noPress: true,
    during: async () => {
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, 400); // the note sounds for 400 emulated ms before the reload starts
      mark.start = (await frames(rt.session)) - trigger;
      const end = await rt.dk.reloadAndWait(MODULE_PATH);
      mark.end = (await frames(rt.session)) - trigger;
      expect(end, '(b) the reload of the machine').toEqual({ result: 0, unloaded: true });
      await expectReloadLeftNothing(rt, plan, '(b) machine with a note held', epoch);
      // the held note belonged to the old module: a new note after the swap is the proof that the new row plays
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
    },
  });
  const pcm = readFloatWav(file);
  const perBlock = pcm.length / length;
  expect(perBlock, `(b) samples per block of the recording (${pcm.length} samples for ${length} blocks)`).toBeGreaterThan(32);
  expect(mark.start, '(b) the reload started after the note began').toBeGreaterThan(100);
  expect(mark.end + 100 + POST_BLOCKS, `(b) the recording (${length} blocks) must hold the whole swap (${mark.start}..${mark.end}) and ${POST_BLOCKS} blocks of the new note after it`).toBeLessThanOrEqual(length);
  const peak = (a: number, b: number) => peakOf(pcm, Math.floor(a * perBlock), Math.ceil(b * perBlock));
  const pre = peak(mark.start - 100, mark.start);
  const swap = windowsOf(pcm, mark.start, mark.end);
  const post = peak(mark.end + 100, mark.end + 100 + POST_BLOCKS);
  expect(pre, `(b) the held note is audible before the reload (peak ${pre.toFixed(4)})`).toBeGreaterThan(0.01);
  // a contiguous run of quiet windows inside the swap (queued .. new note), not one dip: at least 3 windows in a row below 1% of the level before
  const quiet = swap.map((p) => p < pre * 0.01);
  let run = 0;
  let best = 0;
  for (const q of quiet) {
    run = q ? run + 1 : 0;
    best = Math.max(best, run);
  }
  // the control: the same note and length, held for the same span, no reload. Its level over the span stays above the threshold, so decay alone cannot make a quiet stretch
  const controlFile = path.join(artifacts, 'control.wav');
  const span = mark.end - mark.start;
  const controlTrigger = (await frames(rt.session)) + 300;
  await captureNote(rt.session, {
    file: controlFile,
    trigger: controlTrigger,
    tag: 'reload control',
    length,
    noPress: true,
    during: async () => {
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, 400);
      await runFrames(rt.session, span);
    },
  });
  const controlPcm = readFloatWav(controlFile);
  const control = windowsOf(controlPcm, mark.start, mark.end);
  const controlPre = peakOf(controlPcm, Math.floor((mark.start - 100) * perBlock), Math.floor(mark.start * perBlock));
  record(artifacts, 'pcm-windows', { start: mark.start, end: mark.end, pre, swap, post, control, controlPre });
  expect(best, `(b) a run of at least 3 quiet windows inside the swap: peaks per window ${swap.map((p) => p.toFixed(4)).join(' ')} against ${pre.toFixed(4)} before`).toBeGreaterThanOrEqual(3);
  expect(Math.min(...control), `(b) the control without a reload stays audible over the same span: peaks ${control.map((p) => p.toFixed(4)).join(' ')} against ${controlPre.toFixed(4)}`).toBeGreaterThan(controlPre * 0.01);
  expect(post, `(b) the new module plays again (peak ${post.toFixed(4)} after, ${pre.toFixed(4)} before)`).toBeGreaterThan(pre * 0.1);
  const again = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  record(artifacts, 'sine-after', again);
  expect(again.types, '(b) the machine type keeps its number').toEqual(sine.types);
  expect(again.rows, '(b) the voice row is the one it had (the placeholder row is taken over)').toBe(sine.rows);

  // (c) refused while the transport plays; the old module stays and nothing is counted
  await rt.ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
  const refused = await rt.dk.reloadAndWait(MODULE_PATH);
  expect(refused, '(c) the transport plays: refused before the unload').toEqual({ result: E_RELOAD_PLAYING, unloaded: false });
  expect((await moduleRows(rt)).find((m) => m.name === 'test/sine')?.types, '(c) the old module is live').toEqual(sine.types);
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the transport stops' } });
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the second STOP resets the transport' } });
  await reloadChecked(rt, plan, MODULE_PATH, '(c) after STOP', epoch);
}

/** Upward zero crossings of the left channel (every second sample of the interleaved stereo PCM) of `xs[from..to)`: the frequency between the first and the last. */
function leftHz(xs: Float32Array, from: number, to: number, rate = 48_000): number {
  const at: number[] = [];
  let prev = xs[from]!;
  for (let i = from + 2; i < to; i += 2) {
    if (prev < 0 && xs[i]! >= 0) at.push((i - from) / 2);
    prev = xs[i]!;
  }
  if (at.length < 3) throw new Error(`fewer than two periods in ${(to - from) / 2} samples`);
  return (rate * (at.length - 1)) / (at.at(-1)! - at[0]!);
}

async function expectReloadPitchSoak(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-pitch-soak');
  const device = rt.session.options.device as 'dt2' | 'dn2';
  const build = (name: string, edits: Parameters<typeof packModuleVariant>[0]['edits']) =>
    packModuleVariant({ moduleDir: 'tests/fixtures/modules/sine', id: 'test/sine', device, edits, out: path.join(artifacts, `${name}.dkm`) });
  // both builds go through the same pack, so the only difference is the constant
  const builds = [fs.readFileSync(await build('sine-base', [])), fs.readFileSync(await build('sine-octave', [SINE_OCTAVE_UP]))];
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT', 'ui_machine_view_info']);
  const epoch = await epochOf(rt);
  const sine = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  // what the image accounts for: a reload that leaks DSP area, arena or a row per cycle shows here only (the contract suite runs over a fake core)
  const start = await rt.dk.capacity();
  await pickSine(rt);
  const amp = AMP_CC[device];
  await sendCc(rt, MIDI_CHANNEL, amp.hold, 127);
  await sendCc(rt, MIDI_CHANNEL, amp.decay, 64);
  for (const cc of SEND_CC[device]) await sendCc(rt, MIDI_CHANNEL, cc, 0);
  const length = CAPTURE_BLOCKS;
  const hzOf = [220, 440]; // NOTE 57 = A3, then an octave up
  const seen: number[] = [];
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    // the card holds the build of this cycle; the build that runs is the other one (the first cycle: the packed fixture, which is the base build)
    const which = (cycle + 1) % 2; // 1 (octave) first, then 0 (base), ...
    await rt.dk.put(MODULE_PATH, builds[which]!);
    const file = path.join(artifacts, `cycle-${cycle}.wav`);
    const trigger = (await frames(rt.session)) + 300;
    const mark = { end: 0 };
    await captureNote(rt.session, {
      file,
      trigger,
      tag: `pitch soak ${cycle}`,
      length,
      noPress: true,
      during: async () => {
        await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
        await waitClock(rt.clock, 400);
        const end = await rt.dk.reloadAndWait(MODULE_PATH);
        mark.end = (await frames(rt.session)) - trigger;
        expect(end, `soak cycle ${cycle}: the reload`).toEqual({ result: 0, unloaded: true });
        await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
        await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      },
    });
    const pcm = readFloatWav(file);
    const perBlock = pcm.length / length;
    const from = Math.floor((mark.end + 150) * perBlock);
    const to = Math.floor((mark.end + 150 + 400) * perBlock);
    expect(to, `soak cycle ${cycle}: the recording holds the note after the swap (swap ended at block ${mark.end} of ${length})`).toBeLessThanOrEqual(pcm.length);
    const hz = leftHz(pcm, from - (from % 2), to);
    seen.push(hz);
    expect(Math.abs(hz / hzOf[which]! - 1), `soak cycle ${cycle}: the pitch after the swap is ${hz.toFixed(1)} Hz, the build just loaded plays ${hzOf[which]} Hz`).toBeLessThan(0.02);
    const m = (await moduleRows(rt)).find((x) => x.name === 'test/sine')!;
    expect(m.types, `cycle ${cycle}: the machine type moved`).toEqual(sine.types);
    expect(m.rows, `cycle ${cycle}: the voice row moved`).toBe(sine.rows);
    expect(m.faults, `cycle ${cycle}: the guard counted a fault`).toBe(0);
    await expectReloadLeftNothing(rt, plan, `soak cycle ${cycle}`, epoch);
    const cap = await rt.dk.capacity();
    for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA'])
      expect(cap[kind]?.used, `soak cycle ${cycle}: ${kind} in use changed`).toBe(start[kind]?.used);
  }
  record(artifacts, 'pitch-soak', { cycles: SOAK_CYCLES, hz: seen, capacity: await rt.dk.capacity() });
}

runtimeTests('RELOAD swaps a code-only module and a machine without a boot, and refuses while the transport plays', expectReloadLive, { dsp: 'sharc' });
runtimeTests('RELOAD between two builds of a machine that differ in one constant plays the pitch of the build just loaded, cycle after cycle', expectReloadPitchSoak, { dsp: 'sharc' });

const PROJECT_BLOCKS = 4000;

describeOn('dt2', `DT2 ${FIRMWARE.dt2} reload: a project that uses the reloaded machine`, () => {
  const run = cardRig('dt2', 'reload-project', {
    dsp: 'sharc',
    roots: RT,
    symbols: CARD_SYMBOLS,
    initial: () => ({}),
    project: true,
    uploads: true,
    relaxFs: true,
  });

  /** A note held 600 emulated ms with the PCM captured: the pitch (left channel) and the peak over blocks 250 .. 700 after the trigger. */
  async function noteOf(rt: TestRuntime, artifacts: string, tag: string): Promise<{ hz: number; peak: number }> {
    const file = path.join(artifacts, `${tag}.wav`);
    const trigger = (await frames(rt.session)) + 300;
    await captureNote(rt.session, {
      file,
      trigger,
      tag,
      length: PROJECT_BLOCKS,
      noPress: true,
      during: async () => {
        await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
        await waitClock(rt.clock, 600);
        await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      },
    });
    const pcm = readFloatWav(file);
    const perBlock = pcm.length / PROJECT_BLOCKS;
    const from = Math.floor(250 * perBlock) & ~1;
    const to = Math.floor(700 * perBlock);
    return { hz: leftHz(pcm, from, to), peak: peakOf(pcm, from, to) };
  }

  test('save a project with the machine, reload the machine, load the project: the same sound', async () => {
    const path0 = moduleStorePath('dt2', 'test/sine');
    const rt = await run.load('project', { modules: ['test/sine'], selection: [path0] });
    const artifacts = liveArtifacts(rt.session, 'reload-project');
    const type = await dynType(rt.session, run.plan, 'test/sine', 'sine');
    await buildPanelProject(rt, [{ track: 1, type, hint: 7 }]);
    expect(await trackType(rt.session, 1), 'track 1 holds the machine').toBe(type);
    await savePanelProject(rt);
    // the capture holds the machine alone: AMP HOLD is NOTE, the stock FX sends are 0
    await sendCc(rt, MIDI_CHANNEL, AMP_CC.dt2.hold, 127);
    await sendCc(rt, MIDI_CHANNEL, AMP_CC.dt2.decay, 64);
    for (const cc of SEND_CC.dt2) await sendCc(rt, MIDI_CHANNEL, cc, 0);
    const before = await noteOf(rt, artifacts, 'before');
    expect(before.peak, `the machine is audible (peak ${before.peak.toFixed(4)})`).toBeGreaterThan(0.01);

    // DkrtBootInfo.reloads (offset 252) moves when a live job finished; it tells this job's end from the state an earlier job left
    const reloads = planSymbol(run.plan, 'DKRT_BOOT') + 252;
    const reloads0 = await word(rt.session, reloads);
    await run.usb(rt, (dk) => dk.reload(path0));
    const end = await pollUntil(rt.clock, async () => ({ st: await run.usb(rt, (dk) => dk.reloadState()), n: await word(rt.session, reloads) }), (v) => v.st.phase === 2 && v.n > reloads0, { withinMs: 20_000, message: 'the reload of the machine finished' });
    expect({ result: end.st.result, unloaded: end.st.unloaded }, 'the reload of the machine').toEqual({ result: 0, unloaded: true });
    await loadPanelProject(rt);
    expect(await trackType(rt.session, 1), 'after the project load track 1 holds the type of the reloaded machine').toBe(type);
    const after = await noteOf(rt, artifacts, 'after');
    record(artifacts, 'project-roundtrip', { before, after });
    expect(Math.abs(after.hz / before.hz - 1), `the pitch after the reload and the project load (${after.hz.toFixed(2)} Hz) is the pitch before (${before.hz.toFixed(2)} Hz)`).toBeLessThan(0.01);
    expect(Math.abs(after.peak / before.peak - 1), `the level after (${after.peak.toFixed(4)}) is the level before (${before.peak.toFixed(4)})`).toBeLessThan(0.05);
    await rt.session.expectNoFault('project round trip of a reloaded machine');
  }, 900_000);
});

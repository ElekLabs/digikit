import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote, frames, pickSine, readFloatWav, runFrames, word } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Reload of a content module (docs/plans/extension-model.md E5): `RELOAD` swaps the code, DSP and keys of a loaded module without a boot.
 *
 * Every reload below is followed by the same checks of what the runtime and the firmware keep: RELOAD_STATE says finished with result 0; no new boot (the epoch);
 * the `reloads` counter of `DKRT_BOOT` went up by exactly one; no module record, row or placeholder is left over (the voice rows in use are those the modules' row masks name:
 * a placeholder row would count in use and be in no mask); no `TABLE_BAD` in the log; and the machine selection view the hook remembered is still there (`dynreg_info` words 48
 * and 49 name the variable and the builder, and the variable is not 0: the rebuild of the list at the end of a reload has a view to rebuild). The rebuilt list itself is not read.
 *
 * - (a) a code-only module (`test/lfo-shape`) reloads.
 * - (b) the machine `test/sine`, with a note held by MIDI over the reload and the PCM captured: the level before the reload is audible, at least 3 windows in a row inside the
 *   swap are below 1% of it (the placeholder row, not a stock sound), a control capture of the same note and span without a reload stays above that threshold over the whole
 *   span (so decay alone cannot make the quiet run), and a new note after the swap is audible again. The peaks per window are in the assertion messages and `pcm-windows.json`.
 *   The machine type keeps its number and its voice row.
 * - (c) the reload is refused while the transport plays (`E_RELOAD_PLAYING`, the old module is live and the counter did not move) and goes through after STOP.
 * - the soak: the reload of the machine, 5 cycles in the gate and 20 in the full tier (`DIGIKIT_TIER=full`), with the same checks each cycle and the capacity counts
 *   unchanged. The pitch switch of the plan's soak (two builds of `test/sine` that differ in one constant) needs a build helper and is not written yet.
 *
 * Run only with the go of the owner (docs/plans/extension-model.md, "Device proof").
 */
const MODULE_PATH = 'modules/sine.dkm';
const SHAPE_PATH = 'modules/lfo-shape.dkm';
const E_RELOAD_PLAYING = loadCode('E_RELOAD_PLAYING');
const SOAK_CYCLES = process.env.DIGIKIT_TIER === 'full' ? 20 : 5;
/** Byte offsets in DkrtBootInfo (modules/core/runtime/abi.toml) and in `dynreg_info` (word 48: the view variable, word 49: the builder). */
const BOOT_RELOADS = 252;
const DYNREG_VIEW_VAR = 4 * 48;
const DYNREG_VIEW_BUILD = 4 * 49;
const MIDI_CHANNEL = 1;
const NOTE = 57;

const moduleRows = async (rt: TestRuntime) => (await rt.dk.modules()).filter((m) => m.name !== DEBUG_MODULE);
const popcount = (n: number) => n.toString(2).replaceAll('0', '').length;

const epochOf = async (rt: TestRuntime): Promise<number> => {
  const e = (await rt.session.status()).epoch;
  if (typeof e !== 'number') throw new Error('daemon status must report a numeric epoch');
  return e;
};

/** What every reload leaves: see the header. `before` is the reload counter read before the reload. */
async function expectReloadLeftNothing(rt: TestRuntime, plan: RuntimePlan, why: string, epoch: number, before: number, bumped = 1): Promise<void> {
  const st = await rt.dk.reloadState();
  expect(st.phase, `${why}: RELOAD_STATE phase (2 = finished)`).toBe(2);
  expect(st.result, `${why}: RELOAD_STATE result`).toBe(0);
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  expect(await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS), `${why}: the reloads counter`).toBe(before + bumped);
  const [cap, mods] = [await rt.dk.capacity(), await moduleRows(rt)];
  const named = mods.reduce((n, m) => n + popcount(m.rows), 0);
  expect(cap['VOICE_ROWS']?.used, `${why}: voice rows in use (the row masks of the modules name ${named}; a placeholder row would be one more)`).toBe(named);
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
  const info = planSymbol(plan, 'dynreg_info');
  const viewVar = await word(rt.session, info + DYNREG_VIEW_VAR);
  expect(viewVar, `${why}: dynreg_info word 48 (the variable that holds the machine selection view)`).not.toBe(0);
  expect(await word(rt.session, info + DYNREG_VIEW_BUILD), `${why}: dynreg_info word 49 (the list builder)`).not.toBe(0);
  expect(await word(rt.session, viewVar), `${why}: the hook remembered the machine selection view`).not.toBe(0);
}

/** One reload of `modulePath`, then the checks of `expectReloadLeftNothing`. */
async function reloadChecked(rt: TestRuntime, plan: RuntimePlan, modulePath: string, why: string, epoch: number): Promise<void> {
  const before = await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS);
  const end = await rt.dk.reloadAndWait(modulePath);
  expect(end, `${why}: the reload of ${modulePath}`).toEqual({ result: 0, unloaded: true });
  await expectReloadLeftNothing(rt, plan, why, epoch, before);
}

const peakOf = (xs: Float32Array, from: number, to: number): number => {
  let p = 0;
  for (let i = Math.max(0, from); i < Math.min(xs.length, to); i++) p = Math.max(p, Math.abs(xs[i]!));
  return p;
};

async function expectReloadLive(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-live');
  await cliInstall(rt.session, 'test/lfo-shape', artifacts, 300_000, [DEBUG_MODULE, 'test/sine']);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT', 'dynreg_info']);
  const epoch = await epochOf(rt);
  const before = await moduleRows(rt);
  record(artifacts, 'modules-before', before);

  // (a) a code-only module
  await reloadChecked(rt, plan, SHAPE_PATH, '(a) code-only module', epoch);
  expect((await moduleRows(rt)).map((m) => m.name).sort(), '(a) the modules are the same').toEqual(before.map((m) => m.name).sort());

  // (b) the machine with DSP, a note held over the reload, PCM captured
  const sine = before.find((m) => m.name === 'test/sine')!;
  await pickSine(rt);
  const length = 1500;
  const WINDOWS = 32;
  const windowsOf = async (file: string, a: number, b: number): Promise<number[]> => {
    const pcm = readFloatWav(file);
    const perFrame = pcm.length / length;
    const step = Math.max(1, Math.floor((b - a) / WINDOWS));
    const peaks: number[] = [];
    for (let f = a; f + step <= b; f += step) peaks.push(peakOf(pcm, Math.floor(f * perFrame), Math.ceil((f + step) * perFrame)));
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
      const counter = await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS);
      const end = await rt.dk.reloadAndWait(MODULE_PATH);
      mark.end = (await frames(rt.session)) - trigger;
      expect(end, '(b) the reload of the machine').toEqual({ result: 0, unloaded: true });
      await expectReloadLeftNothing(rt, plan, '(b) machine with a note held', epoch, counter);
      // the held note belonged to the old module: a new note after the swap is the proof that the new row plays
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
    },
  });
  const pcm = readFloatWav(file);
  const perFrame = pcm.length / length;
  const peak = (a: number, b: number) => peakOf(pcm, Math.floor(a * perFrame), Math.ceil(b * perFrame));
  const pre = peak(Math.max(0, mark.start - 100), mark.start);
  const swap = await windowsOf(file, mark.start, mark.end);
  const post = peak(mark.end + 100, length);
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
  const control = await windowsOf(controlFile, mark.start, mark.end);
  const controlPre = peakOf(readFloatWav(controlFile), 0, 100 * (readFloatWav(controlFile).length / length));
  record(artifacts, 'pcm-windows', { start: mark.start, end: mark.end, pre, swap, post, control, controlPre });
  expect(best, `(b) a run of at least 3 quiet windows inside the swap: peaks per window ${swap.map((p) => p.toFixed(4)).join(' ')} against ${pre.toFixed(4)} before`).toBeGreaterThanOrEqual(3);
  expect(Math.min(...control), `(b) the control without a reload stays audible over the same span: peaks ${control.map((p) => p.toFixed(4)).join(' ')} against ${controlPre.toFixed(4)}`).toBeGreaterThan(controlPre * 0.01);
  expect(post, `(b) the new module plays again (peak ${post.toFixed(4)} after, ${pre.toFixed(4)} before)`).toBeGreaterThan(pre * 0.1);
  const again = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  record(artifacts, 'sine-after', again);
  expect(again.types, '(b) the machine type keeps its number').toEqual(sine.types);
  expect(again.rows, '(b) the voice row is the one it had (the placeholder row is taken over)').toBe(sine.rows);

  // (c) refused while the transport plays; the old module stays and nothing is counted
  const counter = await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS);
  await rt.ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
  const refused = await rt.dk.reloadAndWait(MODULE_PATH);
  expect(refused, '(c) the transport plays: refused before the unload').toEqual({ result: E_RELOAD_PLAYING, unloaded: false });
  expect((await moduleRows(rt)).find((m) => m.name === 'test/sine')?.types, '(c) the old module is live').toEqual(sine.types);
  expect(await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS), '(c) a refusal is not a reload').toBe(counter);
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the transport stops' } });
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the second STOP resets the transport' } });
  await reloadChecked(rt, plan, MODULE_PATH, '(c) after STOP', epoch);
}

async function expectReloadSoak(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-soak');
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT', 'dynreg_info']);
  const epoch = await epochOf(rt);
  const start = await rt.dk.capacity();
  const sine = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    await reloadChecked(rt, plan, MODULE_PATH, `soak cycle ${cycle}`, epoch);
    const cap = await rt.dk.capacity();
    const m = (await moduleRows(rt)).find((x) => x.name === 'test/sine')!;
    expect(m.types, `cycle ${cycle}: the machine type moved`).toEqual(sine.types);
    expect(m.rows, `cycle ${cycle}: the voice row moved`).toBe(sine.rows);
    expect(m.faults, `cycle ${cycle}: the guard counted a fault`).toBe(0);
    for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
      expect(cap[kind]?.used, `cycle ${cycle}: ${kind} in use grew`).toBe(start[kind]?.used);
    }
  }
  record(artifacts, 'soak', { cycles: SOAK_CYCLES, capacity: await rt.dk.capacity() });
}

runtimeTests('RELOAD swaps a code-only module and a machine without a boot, and refuses while the transport plays', expectReloadLive, { dsp: 'sharc' });
runtimeTests('RELOAD of a machine leaves no row, record or byte behind over many cycles', expectReloadSoak, { dsp: 'sharc' });

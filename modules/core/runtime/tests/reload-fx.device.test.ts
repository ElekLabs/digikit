import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { analysisJson, captureNote, frames, pickSine, readFloatWav, word } from '@digi/regression/runtime/fx-live.ts';
import { pageKit } from '@digi/regression/runtime/fx-pages-ui.ts';
import type { FxProfile } from '@digi/regression/runtime/fx-session.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Reload, unload and load of an effect module while the device runs (docs/plans/extension-model.md E5b): the fx service tears an effect down on any route, so the effect modules
 * `test/dyn-fx-gain` (master route, GAIN default 0.5, MIX wet) and `test/dyn-fx-gain-send` (starts as a send) are no longer refused with `E_RELOAD_ROUTED`.
 *
 * After every job: RELOAD_STATE says finished with result 0; no new boot (the epoch); the `reloads` counter moved by one; no `TABLE_BAD` in the log; FX_SLOTS and SEND_SLOTS in use are what the
 * state of the module says (1 while it is loaded, 0 after an unload). AMP HOLD is NOTE and the stock FX sends are 0, so a capture holds the voice and the run-time effect only.
 *
 * Test 1 (master route, `test/sine` on the track, a note held 600 emulated ms per capture; the levels are peaks of the left and right channels over the span 250 .. 700 blocks after the trigger):
 * - the level with the effect is `wet`; after `UNLOAD` it is `dry` and `dry / wet` is 2 within 5% (the gain 0.5: the row is gone, the chain passes the signal); after `LOAD` the level is `wet` again within 3%
 *   (the DSP row, the page and the knob come back).
 * - the GAIN knob is set in the kit's value array to the raw value 0x1900 (half the default 0x3200: the level halves within 10%); the effect is reloaded with a note held: the PCM windows inside the swap
 *   never fall below 30% of the level before (the chain passes the dry signal while the row is gone, which is louder, not silent), the level of a new note after the swap is the level before within 5%, and the
 *   array still holds 0x1900 (the user's value survives the reload). An unload and a load keep it too, as long as no kit was loaded in between.
 * - the soak: the reload of the effect, 5 cycles in the gate and 20 in the full tier (`DIGIKIT_TIER=full`): the capacity counts do not grow, the level is the same at the end.
 *
 * Test 2 (send route, no audio): the effect starts as a send, so a send cell is in use. `UNLOAD` gives the cell back (SEND_SLOTS and FX_SLOTS 0), `LOAD` takes it again, a reload keeps it; the fx route setting
 * is not written by any of them.
 *
 * Written, not run (docs/plans/extension-model.md, "Device proof").
 */
const MASTER_ID = 'test/dyn-fx-gain';
const MASTER_PATH = 'modules/dyn-fx-gain.dkm';
const SEND_ID = 'test/dyn-fx-gain-send';
const SEND_PATH = 'modules/dyn-fx-gain-send.dkm';
const SOAK_CYCLES = process.env.DIGIKIT_TIER === 'full' ? 20 : 5;
/** Byte offsets in DkrtBootInfo (modules/core/runtime/abi.toml). */
const BOOT_RELOADS = 252;
const MIDI_CHANNEL = 1;
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
const NOTE = 57;
const CAPTURE_BLOCKS = 8000;
/** The raw value of the GAIN knob: half the default 0.5 (0x3200 of 0x7f00). */
const HALF_RAW = 0x1900;

const epochOf = async (rt: TestRuntime): Promise<number> => {
  const e = (await rt.session.status()).epoch;
  if (typeof e !== 'number') throw new Error('daemon status must report a numeric epoch');
  return e;
};

/** The largest magnitude of `xs[from..to)`; a window outside the recording is an error (an empty window would read as silence). */
const peakOf = (xs: Float32Array, from: number, to: number): number => {
  if (from < 0 || to > xs.length || to <= from) throw new Error(`window ${from}..${to} is outside the PCM of ${xs.length} samples`);
  let p = 0;
  for (let i = from; i < to; i++) p = Math.max(p, Math.abs(xs[i]!));
  return p;
};

type Op = 'reload' | 'unload' | 'load';

/** One live job on `file`, then what every job leaves (see the header). `counted` is whether the `reloads` counter moves (a load and a reload do, an unload moves `unloads` only). */
async function job(rt: TestRuntime, plan: RuntimePlan, op: Op, file: string, why: string, epoch: number): Promise<void> {
  const before = await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS);
  const end = await (op === 'reload' ? rt.dk.reloadAndWait(file) : op === 'unload' ? rt.dk.unloadAndWait(file) : rt.dk.loadAndWait(file));
  expect(end, `${why}: ${op} of ${file}`).toEqual({ result: 0, unloaded: op !== 'load' });
  const st = await rt.dk.reloadState();
  expect([st.phase, st.result], `${why}: RELOAD_STATE (phase 2 = finished, result 0)`).toEqual([2, 0]);
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  expect(await word(rt.session, planSymbol(plan, 'DKRT_BOOT') + BOOT_RELOADS), `${why}: the reloads counter`).toBe(before + (op === 'unload' ? 0 : 1));
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
}

/** FX_SLOTS and SEND_SLOTS in use. */
async function slots(rt: TestRuntime): Promise<{ fx: number; send: number }> {
  const cap = await rt.dk.capacity();
  return { fx: cap['FX_SLOTS']?.used ?? -1, send: cap['SEND_SLOTS']?.used ?? -1 };
}

async function prepare(rt: TestRuntime, name: string, fxId: string) {
  const artifacts = liveArtifacts(rt.session, name);
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE, fxId]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['DKRT_BOOT']);
  const device = rt.session.options.device as 'dt2' | 'dn2';
  await pickSine(rt);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device].decay, 64);
  for (const cc of SEND_CC[device]) await sendCc(rt, MIDI_CHANNEL, cc, 0);
  return { artifacts, plan, epoch: await epochOf(rt), device };
}

async function expectFxMaster(rt: TestRuntime): Promise<void> {
  const { artifacts, plan, epoch, device } = await prepare(rt, 'reload-fx-master', MASTER_ID);
  expect(await slots(rt), 'the effect is loaded: one effect row, no send cell').toEqual({ fx: 1, send: 0 });

  /** A note held 600 ms with the PCM captured: the peak over 250 .. 700 blocks after the trigger. */
  const level = async (tag: string): Promise<number> => {
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
        await waitClock(rt.clock, 600);
        await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      },
    });
    const pcm = readFloatWav(file);
    const perBlock = pcm.length / CAPTURE_BLOCKS;
    return peakOf(pcm, Math.floor(250 * perBlock), Math.floor(700 * perBlock));
  };

  const wet = await level('wet');
  expect(wet, `the effect passes the machine (peak ${wet.toFixed(4)})`).toBeGreaterThan(0.01);
  await job(rt, plan, 'unload', MASTER_PATH, 'unload', epoch);
  expect(await slots(rt), 'after the unload: no effect row').toEqual({ fx: 0, send: 0 });
  const dry = await level('dry');
  expect(dry / wet, `the machine alone is twice the level of the 0.5 gain (dry ${dry.toFixed(4)}, wet ${wet.toFixed(4)})`).toBeGreaterThan(1.9);
  expect(dry / wet).toBeLessThan(2.1);
  await job(rt, plan, 'load', MASTER_PATH, 'load', epoch);
  expect(await slots(rt), 'after the load: the effect row is back').toEqual({ fx: 1, send: 0 });
  const wet2 = await level('wet-again');
  expect(Math.abs(wet2 / wet - 1), `the level after the load (${wet2.toFixed(4)}) is the level before (${wet.toFixed(4)})`).toBeLessThan(0.03);

  // the GAIN knob in the kit's array: the user's value stays through a reload, an unload and a load
  const profile = await analysisJson<FxProfile>('tools.patchkit.pk_digikit_fxroutes', ['profile', '--device', device], artifacts, 'profile');
  const kit = pageKit(rt, profile);
  await kit.poke(profile.fx.value_indices[0]!, HALF_RAW);
  await rt.ui.wait({ ms: 200, why: 'the UI loop sends the changed knob to the DSP row (no observable state of the transfer)' });
  const half = await level('half');
  expect(half / wet, `the knob at ${HALF_RAW.toString(16)} halves the level (${half.toFixed(4)} against ${wet.toFixed(4)})`).toBeGreaterThan(0.45);
  expect(half / wet).toBeLessThan(0.55);

  // the reload with a note held: the windows inside the swap are never silent
  const file = path.join(artifacts, 'swap.wav');
  const trigger = (await frames(rt.session)) + 300;
  const mark = { start: 0, end: 0 };
  await captureNote(rt.session, {
    file,
    trigger,
    tag: 'effect reload swap',
    length: CAPTURE_BLOCKS,
    noPress: true,
    during: async () => {
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
      await waitClock(rt.clock, 400);
      mark.start = (await frames(rt.session)) - trigger;
      await job(rt, plan, 'reload', MASTER_PATH, 'reload with a note held', epoch);
      mark.end = (await frames(rt.session)) - trigger;
      await sendNoteOff(rt, MIDI_CHANNEL, NOTE);
      await sendNote(rt, MIDI_CHANNEL, NOTE, 100);
    },
  });
  const pcm = readFloatWav(file);
  const perBlock = pcm.length / CAPTURE_BLOCKS;
  expect(mark.end + 500, `the recording holds the swap (${mark.start}..${mark.end}) and the new note`).toBeLessThanOrEqual(CAPTURE_BLOCKS);
  const windows: number[] = [];
  const step = Math.max(1, Math.floor((mark.end - mark.start) / 16));
  for (let f = mark.start; f + step <= mark.end; f += step) windows.push(peakOf(pcm, Math.floor(f * perBlock), Math.ceil((f + step) * perBlock)));
  record(artifacts, 'swap-windows', { start: mark.start, end: mark.end, windows, half });
  expect(Math.min(...windows), `no window of the swap is silent: peaks ${windows.map((p) => p.toFixed(4)).join(' ')} against ${half.toFixed(4)} before`).toBeGreaterThan(half * 0.3);
  const after = peakOf(pcm, Math.floor((mark.end + 150) * perBlock), Math.floor((mark.end + 450) * perBlock));
  expect(Math.abs(after / half - 1), `the level after the reload (${after.toFixed(4)}) is the level before (${half.toFixed(4)}): the knob value survived`).toBeLessThan(0.05);
  expect(await kit.raw(0), 'the kit array still holds the user value').toBe(HALF_RAW);
  expect(await slots(rt), 'after the reload: one effect row').toEqual({ fx: 1, send: 0 });

  // an unload and a load keep it too (no kit was loaded in between)
  await job(rt, plan, 'unload', MASTER_PATH, 'unload with the knob turned', epoch);
  await job(rt, plan, 'load', MASTER_PATH, 'load with the knob turned', epoch);
  expect(await kit.raw(0), 'the value after an unload and a load').toBe(HALF_RAW);
  await rt.ui.wait({ ms: 200, why: 'the UI loop sends the knob to the new DSP row (no observable state of the transfer)' });
  const half2 = await level('half-again');
  expect(Math.abs(half2 / half - 1), `the level after the unload and the load (${half2.toFixed(4)}) is the level before (${half.toFixed(4)})`).toBeLessThan(0.05);

  // the soak
  const start = await rt.dk.capacity();
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    await job(rt, plan, 'reload', MASTER_PATH, `soak cycle ${cycle}`, epoch);
    const cap = await rt.dk.capacity();
    for (const kind of ['MODULES', 'FX_SLOTS', 'SEND_SLOTS', 'DSP_AREA', 'ARENA']) {
      expect(cap[kind]?.used, `cycle ${cycle}: ${kind} in use grew`).toBe(start[kind]?.used);
    }
  }
  const end = await level('after-soak');
  expect(Math.abs(end / half - 1), `the level after the soak (${end.toFixed(4)}) is the level before (${half.toFixed(4)})`).toBeLessThan(0.05);
  record(artifacts, 'soak', { cycles: SOAK_CYCLES, capacity: await rt.dk.capacity() });
}

async function expectFxSend(rt: TestRuntime): Promise<void> {
  const { artifacts, plan, epoch } = await prepare(rt, 'reload-fx-send', SEND_ID);
  expect(await slots(rt), 'the effect starts as a send: a send cell is in use').toEqual({ fx: 1, send: 1 });
  await job(rt, plan, 'unload', SEND_PATH, 'send effect unload', epoch);
  expect(await slots(rt), 'after the unload the send cell and the effect row are given back').toEqual({ fx: 0, send: 0 });
  await job(rt, plan, 'load', SEND_PATH, 'send effect load', epoch);
  expect(await slots(rt), 'after the load the route comes back from the settings').toEqual({ fx: 1, send: 1 });
  await job(rt, plan, 'reload', SEND_PATH, 'send effect reload', epoch);
  expect(await slots(rt), 'after the reload').toEqual({ fx: 1, send: 1 });
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    await job(rt, plan, 'unload', SEND_PATH, `send soak cycle ${cycle}`, epoch);
    await job(rt, plan, 'load', SEND_PATH, `send soak cycle ${cycle}`, epoch);
    expect(await slots(rt), `cycle ${cycle}: the send cell and the row`).toEqual({ fx: 1, send: 1 });
  }
  record(artifacts, 'send-soak', { cycles: SOAK_CYCLES, capacity: await rt.dk.capacity() });
}

runtimeTests('RELOAD, UNLOAD and LOAD of an effect on the master route: the level, the knob value, a note held over the swap, the soak', expectFxMaster, { dsp: 'sharc' });
runtimeTests('RELOAD, UNLOAD and LOAD of an effect that starts as a send: the send cell is given back and comes back', expectFxSend, { dsp: 'sharc' });

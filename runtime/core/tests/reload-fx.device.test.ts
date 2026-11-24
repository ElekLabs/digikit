import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { button, waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { REDRAW, SETTLE, bounded, captureNote, frames, pickSine, readFloatWav, rpc, stableFrameHash, word } from '@digi/regression/runtime/fx-live.ts';
import { createFxKitUi } from '@digi/regression/runtime/fx-kit-ui.ts';
import { kitMenu, kitSlotOne } from '@digi/regression/runtime/fx-track-ui.ts';
import { serviceSymbol } from '@digi/regression/runtime/service-symbols.ts';
import { effectPage, mainPage, pageKit } from '@digi/regression/runtime/fx-pages-ui.ts';
import { wordBase } from '@digi/regression/runtime/memory.ts';
import { fxProfile } from '@digi/regression/runtime/device-data.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Reload, unload and load of an effect module while the device runs (docs/plans/extension-model.md E5b): the fx service tears an effect down on any route, so the effect modules
 * `test/dyn-fx-gain` (master route, GAIN default 0.5, MIX wet) and `test/dyn-fx-gain-send` (starts as a send) are no longer refused with `E_RELOAD_ROUTED`.
 *
 * After every job: the job ends with result 0; no new boot (the epoch); no `TABLE_BAD` in the log. AMP HOLD is NOTE and the stock FX sends are 0, so a
 * capture holds the voice and the run-time effect only.
 *
 * After every job FX_SLOTS and SEND_SLOTS in use are read (two capacity reads): 1 and the module's send cell (0 or 1) while it is loaded, 0 and 0 after an unload. Only the device shows a real effect
 * row or send cell that leaks in a reload (the real loader's `dsp_clear` and `fxsend::remove`); the contract suite of the fx service runs a FakeCore.
 *
 * The test shows what only the firmware shows: the audio of the effect and of the send bus, the capacity counts of the real loader, the DSP row taking the knob value, the swap windows and the page
 * frame. The rest is a unit test and not asserted here: the residue of an unload and the soak of many reloads (the contract suite of the fx service, `runtime/core/test-support/src/contract.rs`:
 * cases 3, 6, 7, 11 and 12; no soak loop here), the parked knob values per module and kit (case 13), the send cell and the page row that come back with a reload
 * (`services/fx/tests/contract.rs` `a_reloaded_effect_gets_its_send_cell_and_page_row_back`), and that no load or unload writes a setting (cases 3 and 7 compare the settings).
 *
 * Test 1 (master route, `test/sine` on the track, a note held 600 emulated ms per capture; the levels are peaks of the left and right channels over the span 250 .. 700 blocks after the trigger):
 * - the level with the effect is `wet`; after `UNLOAD` it is `dry` and `dry / wet` is 2 within 5% (the gain 0.5: the row is gone, the chain passes the signal); after `LOAD` the level is `wet` again within 3%
 *   (the DSP row, the page and the knob come back).
 * - the GAIN knob is set in the kit's value array to the raw value 0x1900 (half the default 0x3200: the level halves within 10%); the effect is reloaded with a note held: the PCM windows inside the swap
 *   never fall below 30% of the level before (the chain passes the dry signal while the row is gone, which is louder, not silent), the
 *   level of a new note after the swap is the level before within 5% (the user's value survives the reload and reaches the DSP row). An unload and a load keep it too, as long as no kit was loaded in
 *   between.
 *
 * - a kit load while the module is gone (the KIT menu, LOAD of a saved slot, between an `UNLOAD` and a `LOAD`): the service's counter of kit loads moves although no row holds an effect, and the level after
 *   the `LOAD` is the default level `wet` within 5%, not the turned one.
 * - the FX page of the effect is open during an `UNLOAD`: no fault, the frame holds still (the page shows the empty row of the unloaded effect, the state of a failed effect load), and after leaving the page and
 *   a `LOAD` the page shows the effect again. Whether the view should instead be put back to the stock list is open (docs/plans/extension-model.md, E5b item 2: no predicate says which view is on top).
 *
 * Test 2 (send route): the effect starts as a send, and the track's send amount (Sound array of track 1, written in the kit) is 127. A note then returns the send bus on top of the
 * dry voice: its level is above the level without the effect by at least 5%. `UNLOAD` gives the cell back and the send bus stops returning audio (the capture is the control for the first one). `LOAD`
 * takes the cell again from the route setting and the return is back, a reload keeps it.
 *
 * Written, not run (docs/plans/extension-model.md, "Device proof").
 */
const MASTER_ID = 'test/dyn-fx-gain';
const SEND_ID = 'test/dyn-fx-gain-send';
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

/** The selection index of the effect module: `prepare` installs `test/sine`, then the debug module, then the effect. */
const FX_INDEX = 2;

type Op = 'reload' | 'unload' | 'load';

/** One live job on `file`, then what every job leaves (see the header). The job's end result is the proof that the live job ran; its state body and the `reloads` counter (one for each live job) are host tests (`reload.rs` `the_live_commands_answer_with_the_jobs_refusal_and_state_in_the_reload_state_body`, `a_job_that_changed_the_module_set_counts_one_reload_and_a_refusal_counts_none`). */
async function job(rt: TestRuntime, _plan: RuntimePlan, op: Op, file: string, why: string, epoch: number, send: 0 | 1): Promise<void> {
  // the effect row is claimed when the module's commit writes its rows (MODULE_COMMIT): the capacity read below waits for that event, not for the job's end
  const events = op === 'unload' ? undefined : await rt.listen();
  const end = await (op === 'reload' ? rt.dk.reloadAndWait(file) : op === 'unload' ? rt.dk.unloadAndWait(file) : rt.dk.loadAndWait(file));
  expect(end, `${why}: ${op} of ${file}`).toEqual({ result: 0, unloaded: op !== 'load' });
  // the event names no module, only its selection index (a live job keeps it): the effect module is the third of the install (the machine, the debug module, the effect)
  await events?.until('MODULE_COMMIT', (e) => Number(e.fields.index) === FX_INDEX, { withinMs: 5000 });
  expect(await epochOf(rt), `${why}: a boot happened`).toBe(epoch);
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
  const cap = await rt.dk.capacity();
  expect({ fx: cap['FX_SLOTS']?.used, send: cap['SEND_SLOTS']?.used }, `${why}: FX_SLOTS and SEND_SLOTS in use`).toEqual(op === 'unload' ? { fx: 0, send: 0 } : { fx: 1, send });
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
  return { artifacts, plan, epoch: await epochOf(rt), device, file: moduleStorePath(device, fxId) };
}

async function expectFxMaster(rt: TestRuntime): Promise<void> {
  const { artifacts, plan, epoch, device, file: MASTER_PATH } = await prepare(rt, 'reload-fx-master', MASTER_ID);

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
  await job(rt, plan, 'unload', MASTER_PATH, 'unload', epoch, 0);
  const dry = await level('dry');
  expect(dry / wet, `the machine alone is twice the level of the 0.5 gain (dry ${dry.toFixed(4)}, wet ${wet.toFixed(4)})`).toBeGreaterThan(1.9);
  expect(dry / wet).toBeLessThan(2.1);
  await job(rt, plan, 'load', MASTER_PATH, 'load', epoch, 0);
  const wet2 = await level('wet-again');
  expect(Math.abs(wet2 / wet - 1), `the level after the load (${wet2.toFixed(4)}) is the level before (${wet.toFixed(4)})`).toBeLessThan(0.03);

  // the GAIN knob in the kit's array: the user's value stays through a reload, an unload and a load
  const profile = fxProfile(device);
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
      await job(rt, plan, 'reload', MASTER_PATH, 'reload with a note held', epoch, 0);
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
  // the level after the reload: a fresh note, measured as `half` was (the frame marks place a window in the swap capture only roughly)
  const after = await level('after-reload');
  expect(Math.abs(after / half - 1), `the level after the reload (${after.toFixed(4)}) is the turned level (${half.toFixed(4)}), not the default (${wet.toFixed(4)}): the knob value survived`).toBeLessThan(0.05);

  // an unload and a load keep it too (no kit was loaded in between)
  await job(rt, plan, 'unload', MASTER_PATH, 'unload with the knob turned', epoch, 0);
  await job(rt, plan, 'load', MASTER_PATH, 'load with the knob turned', epoch, 0);
  await rt.ui.wait({ ms: 200, why: 'the UI loop sends the knob to the new DSP row (no observable state of the transfer)' });
  const half2 = await level('half-again');
  expect(Math.abs(half2 / half - 1), `the level after the unload and the load (${half2.toFixed(4)}) is the level before (${half.toFixed(4)})`).toBeLessThan(0.05);

  // the FX page of the effect open during an unload
  await effectPage(rt);
  await rt.ui.wait({ ms: 400, why: 'the effect page is drawn (no observable state of the redraw)' });
  const shown = await stableFrameHash(rt);
  await job(rt, plan, 'unload', MASTER_PATH, 'unload with the effect page open', epoch, 0);
  await rt.ui.wait({ ms: 400, why: 'the view redraws after the unload (no observable state of the redraw)' });
  const emptied = await stableFrameHash(rt);
  record(artifacts, 'page-open-unload', { shown, emptied });
  await rt.session.expectNoFault('the effect page open during an unload');
  await mainPage(rt);
  await job(rt, plan, 'load', MASTER_PATH, 'load after the page was open', epoch, 0);
  await effectPage(rt);
  await rt.ui.wait({ ms: 400, why: 'the effect page is drawn (no observable state of the redraw)' });
  expect(await stableFrameHash(rt), 'the page shows the effect again after the load').toBe(shown);
  await mainPage(rt);

  // a kit load while the module is gone: the parked values are not the new kit's, so the load gives the defaults (the real kit-load hook of the fx service counts the load
  // although no row holds an effect: `fxp_load` in services/fx/dks/src/hooks.rs bumps DKRT_FXP_NLOAD before it looks at any row)
  await createFxKitUi(rt, plan, profile, artifacts).saveKit(2, 'other-kit'); // the kit holds the turned value now, in a slot to load back
  await job(rt, plan, 'unload', MASTER_PATH, 'unload before a kit load', epoch, 0);
  const nload = await serviceSymbol(rt, plan, 'fx', 'DKRT_FXP_NLOAD');
  const loads = await word(rt.session, nload);
  await loadKitSlot(rt, 2);
  expect(await word(rt.session, nload), 'the firmware kit loader ran (the service counted it with no effect loaded)').toBeGreaterThan(loads);
  await job(rt, plan, 'load', MASTER_PATH, 'load after a kit load', epoch, 0);
  await rt.ui.wait({ ms: 200, why: 'the UI loop sends the knob to the new DSP row (no observable state of the transfer)' });
  const afterKit = await level('after-kit-load');
  expect(Math.abs(afterKit / wet - 1), `a kit was loaded while the module was gone: the default level (${afterKit.toFixed(4)} against ${wet.toFixed(4)}), not the turned one (${half.toFixed(4)})`).toBeLessThan(0.05);
}

/** The KIT menu's LOAD of kit slot `n` with the held YES key, as `createFxKitUi.loadKit` does it, without the DSP row watch. */
async function loadKitSlot(rt: TestRuntime, n: number): Promise<void> {
  await kitMenu(rt, 0);
  await rt.ui.tap('YES', { until: { ms: 700, why: REDRAW } });
  await kitSlotOne(rt);
  for (let i = 1; i < n; i++) await rt.ui.tap('DOWN', { until: { ms: 80, why: REDRAW } });
  try {
    await bounded(() => rt.session.client.inputButton(button('YES'), true));
    await rt.ui.wait({ ms: 60, why: SETTLE });
    await bounded(() => rt.session.client.inputButton(button('YES'), false));
  } finally {
    await bounded(() => rt.session.client.inputButton(button('YES'), false));
  }
  await rt.ui.wait({ ms: 600, why: 'the kit loader runs and the firmware redraws (the load counter is read after)' });
}

/** Write the halfword `value` at the firmware address `at` (the kit objects are in DDR; the write goes word by word through the daemon, as `pageKit.poke` does). */
async function pokeHalf(rt: TestRuntime, at: number, value: number): Promise<void> {
  for (let i = 0; i < 2; i++) {
    const byte = at + i;
    const shift = 8 * (3 - (byte % 4));
    const old = await word(rt.session, wordBase(byte));
    await rpc(rt.session, 'mem.cf.poke', {
      addr: wordBase(byte),
      value: ((old & ~(0xff << shift)) | (((value >>> (8 * (1 - i))) & 0xff) << shift)) >>> 0,
    });
  }
}

async function expectFxSend(rt: TestRuntime): Promise<void> {
  const { artifacts, plan, epoch, device, file: SEND_PATH } = await prepare(rt, 'reload-fx-send', SEND_ID);
  const profile = fxProfile(device);
  // the send amount of track 1 for cell 0 is the Sound array value `amount << 8`, as the firmware's knob writes it; the service reads it as a turn
  const sound = (await word(rt.session, profile.fx.kit_ptr)) + profile.fx.sound_off;
  await pokeHalf(rt, sound + 0x14 + 2 * profile.fx.send_indices[0]!, 127 << 8);
  await rt.ui.wait({ ms: 300, why: 'the UI loop reads the amount and hands it to the DSP row (no observable state of the transfer)' });

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
  const withSend = await level('send-on');
  await job(rt, plan, 'unload', SEND_PATH, 'send effect unload', epoch, 1);
  const dry = await level('send-unloaded');
  expect(withSend / dry, `the send bus returns audio on top of the dry voice (${withSend.toFixed(4)} against ${dry.toFixed(4)} without the effect)`).toBeGreaterThan(1.05);
  await job(rt, plan, 'load', SEND_PATH, 'send effect load', epoch, 1);
  await rt.ui.wait({ ms: 300, why: 'the amount goes to the new DSP row (no observable state of the transfer)' });
  const back = await level('send-loaded');
  expect(Math.abs(back / withSend - 1), `the return is back after the load (${back.toFixed(4)} against ${withSend.toFixed(4)})`).toBeLessThan(0.05);
  await job(rt, plan, 'reload', SEND_PATH, 'send effect reload', epoch, 1);
  await rt.ui.wait({ ms: 300, why: 'the amount goes to the new DSP row (no observable state of the transfer)' });
  const reloaded = await level('send-reloaded');
  expect(Math.abs(reloaded / withSend - 1), `the return after the reload (${reloaded.toFixed(4)} against ${withSend.toFixed(4)})`).toBeLessThan(0.05);
  record(artifacts, 'send', { withSend, dry, back, reloaded });
}

runtimeTests('RELOAD, UNLOAD and LOAD of an effect on the master route: the level, the knob value, a note held over the swap, the page open', expectFxMaster, { dsp: 'sharc' });
runtimeTests('RELOAD, UNLOAD and LOAD of an effect that starts as a send: the send cell is given back and comes back', expectFxSend, { dsp: 'sharc' });

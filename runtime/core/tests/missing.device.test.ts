import path from 'node:path';
import { pollUntil, waitClock, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote } from '@digi/regression/runtime/note-capture.ts';
import { frames } from '@digi/regression/runtime/dsp-state.ts';
import { pickSine } from '@digi/regression/runtime/panel-waits.ts';
import { readFloatWav } from '@digi/regression/runtime/pcm-files.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { TRACK_OBJ_WORD } from '@digi/regression/runtime/project-kit.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { bootInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall, transferCommand } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * An absent machine module reads MISSING and plays silence (docs/plans/extension-model.md E5d), on both devices. The machine is `test/sine` (the machine the other live tests use;
 * the plan names the pluck machine, whose pitch report needs the sample pool: the sine has the same row lifecycle).
 *
 * Before the unload the machine is in the selection list (its row has a list name). `UNLOAD` of the machine with the module outside the saved selection:
 * - the machine row's short name (the text the track label draws) is `MISS`, and the machine is not in the selection list (the row's list name is the empty string);
 * - a note on the track renders digital silence: the peak over the held note is below 1e-4 (the placeholder voice row: DT2 through the disabled word, DN2 through the row's own number
 *   that no engine has), against the audible note before the unload;
 * - `LOAD` of it again: the short name is the machine's own, the note plays at the level it had (at least half), and the kit's stored knob bytes of track 1 (the sound array, DT2:
 *   `+0x14`, 0x8c bytes of the track object) are the bytes before the unload: the unload and the load wrote nothing to the track.
 *
 * Boot path: the machines service is a provider that loads only when a selected module needs it, so a boot with the debug module alone loads no machines service, reads no
 * sticky map and writes no row. The boot path therefore runs with a second machine module (`test/native-page`) selected beside the debug module: the machines service loads, the
 * number of `test/sine` is still in the map (the map keeps the numbers of modules that are not selected) and no module has it. After the restart, once the loads of the boot are over,
 * the service parks it, so the row's short name reads `MISS` (waited on firmware state) and the log holds no `MACHINE_TYPE_EVICTED` error (no number was left without a row). The
 * silence of that row is the unit test of the service and the rig case (a track cannot hold the type after a restart without a saved project). Then a live load of `test/sine`
 * takes the parked row back and the machine is its own again (a restart before the stable period ends would start a safe boot, so the test does not restart twice).
 *
 * Replaces the "a plain unload leaves a gap row that plays the first stock machine" assertions of `live.device.test.ts`.
 *
 * Written, not run (docs/plans/extension-model.md, "Device proof").
 */
const MIDI_CHANNEL = 1;
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
/** Where the kit's stored knob bytes of track 1 can be read (the sound array, finding 02): only the Digitakt II track object is mapped in the tests. */
const KNOB_BYTES: Partial<Record<Device, { at: number; words: number }>> = { dt2: { at: TRACK_OBJ_WORD + 0x14, words: 0x8c / 4 } };
/** A second machine module that keeps the machines service loaded while `test/sine` is not selected. */
const OTHER_ID = 'test/native-page';
const NOTE = 57;
const CAPTURE_BLOCKS = 4000;
const SILENT = 1e-4;

/** The NUL-terminated string at `addr`, read by words. */
async function cstr(rt: TestRuntime, addr: number): Promise<string> {
  const out: number[] = [];
  for (let k = 0; k < 40; k += 4) {
    const w = await word(rt.session, addr + k);
    out.push(w >>> 24, (w >>> 16) & 255, (w >>> 8) & 255, w & 255);
    if (out.includes(0)) break;
  }
  const end = out.indexOf(0);
  return Buffer.from(end < 0 ? out : out.slice(0, end)).toString('latin1');
}

/** Row layout from `rows_info` (words: M_ROWS 3, M_ROW_SIZE 4, M_FIRST 5, O_NAME 18, O_SNAME 19, EMPTY 29): the machine row of `type`. */
const INFO = { rows: 3, size: 4, first: 5, name: 18, sname: 19, empty: 29 } as const;

async function machineRow(rt: TestRuntime, plan: RuntimePlan, type: number) {
  const info = planSymbol(plan, 'rows_info');
  const w = (i: number) => word(rt.session, info + 4 * i);
  const row = (await w(INFO.rows)) + ((await w(INFO.size)) * (type - (await w(INFO.first))));
  return { row, name: await w(INFO.name), sname: await w(INFO.sname), empty: await w(INFO.empty) };
}

/** The short name of the machine row of `type` (the text the page and the track label draw). */
async function shortName(rt: TestRuntime, plan: RuntimePlan, type: number): Promise<string> {
  const r = await machineRow(rt, plan, type);
  return cstr(rt, await word(rt.session, r.row + r.sname));
}

/** Whether the machine row of `type` has a list name (the selection list shows it). */
async function listed(rt: TestRuntime, plan: RuntimePlan, type: number): Promise<boolean> {
  const r = await machineRow(rt, plan, type);
  return (await word(rt.session, r.row + r.name)) !== r.empty;
}

/** The peak of the left channel over a held note (blocks 250 .. 700 after the trigger). */
async function notePeak(rt: TestRuntime, artifacts: string, tag: string): Promise<number> {
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
  let peak = 0;
  for (let i = Math.floor(250 * perBlock); i < Math.floor(700 * perBlock); i++) peak = Math.max(peak, Math.abs(pcm[i]!));
  return peak;
}

async function trackWords(rt: TestRuntime, device: Device): Promise<number[] | undefined> {
  const where = KNOB_BYTES[device];
  if (!where) return undefined;
  return Promise.all(Array.from({ length: where.words }, (_, i) => word(rt.session, where.at + 4 * i)));
}

async function expectMissingAndSilent(rt: TestRuntime): Promise<void> {
  const device = rt.session.options.device as Device;
  const artifacts = liveArtifacts(rt.session, 'missing');
  const sinePath = moduleStorePath(device as 'dt2' | 'dn2', 'test/sine');
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, ['rows_info', 'DKRT_BOOT']);
  // the next boot would load the debug module alone: the machine is outside the saved selection
  await rt.dk.select([moduleStorePath(device as 'dt2' | 'dn2', DEBUG_MODULE)]);
  await transferCommand(rt.session, ['install', OTHER_ID], path.join(artifacts, 'install-other.log'), 300_000);
  const sine = (await rt.dk.modules()).find((m) => m.name === 'test/sine')!;
  const type = sine.types[0]!;
  await pickSine(rt);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device as 'dt2' | 'dn2'].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device as 'dt2' | 'dn2'].decay, 64);
  for (const cc of SEND_CC[device as 'dt2' | 'dn2']) await sendCc(rt, MIDI_CHANNEL, cc, 0);

  expect(await listed(rt, plan, type), 'the loaded machine is in the selection list').toBe(true);
  const heard = await notePeak(rt, artifacts, 'before');
  expect(heard, `the machine is audible (peak ${heard.toFixed(4)})`).toBeGreaterThan(0.01);
  const ownShort = await shortName(rt, plan, type);
  expect(ownShort, 'the loaded machine has its own short name').not.toBe('MISS');
  const stored = await trackWords(rt, device);

  const gone = await rt.dk.unloadAndWait(sinePath);
  expect(gone, 'the unload').toEqual({ result: 0, unloaded: true });
  expect(await shortName(rt, plan, type), 'the track label of the absent machine').toBe('MISS');
  expect(await listed(rt, plan, type), 'the absent machine is not in the selection list').toBe(false);
  const silent = await notePeak(rt, artifacts, 'absent');
  record(artifacts, 'missing-pcm', { heard, silent });
  expect(silent, `a note on the absent machine is silent (peak ${silent.toExponential(2)})`).toBeLessThan(SILENT);
  await rt.session.expectNoFault('a note on the absent machine');
  expect((await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.TABLE_BAD), 'a table check failed').toEqual([]);

  const back = await rt.dk.loadAndWait(sinePath);
  expect(back, 'the load').toEqual({ result: 0, unloaded: false });
  expect(await shortName(rt, plan, type), 'the machine has its own short name again').toBe(ownShort);
  const again = await notePeak(rt, artifacts, 'after');
  expect(again, `the machine plays again (peak ${again.toFixed(4)} against ${heard.toFixed(4)} before)`).toBeGreaterThan(heard * 0.5);
  const after = await trackWords(rt, device);
  if (stored) expect(after, "the kit's stored knob bytes are the bytes before the unload (the load restores the sound only)").toEqual(stored);

  // the boot path: the selection is the debug module alone, the machine's number stays in the sticky map, and the machines service parks it once the boot's loads are over
  await rt.dk.unloadAndWait(sinePath);
  await rt.dk.select([moduleStorePath(device as 'dt2' | 'dn2', DEBUG_MODULE), moduleStorePath(device as 'dt2' | 'dn2', OTHER_ID)]);
  await restart(rt, plan);
  const parked = await pollUntil(rt.clock, () => shortName(rt, plan, type), (n) => n === 'MISS', { withinMs: 30_000, message: 'the absent machine reads MISS after the restart' });
  expect(parked).toBe('MISS');
  expect(await listed(rt, plan, type), 'the absent machine is not in the selection list after the restart').toBe(false);
  const errors = (await rt.dk.readLog()).filter((e) => e.kind === LOG_KIND.MACHINE_TYPE_EVICTED);
  expect(errors, 'no number was left without a row (MACHINE_TYPE_EVICTED)').toEqual([]);
  // the placeholder parked at boot gives its row back to a live load of the machine: the row is its own again
  expect(await rt.dk.loadAndWait(sinePath), 'the live load of the machine after the boot').toEqual({ result: 0, unloaded: false });
  expect(await shortName(rt, plan, type), 'the machine is its own again after the load').toBe(ownShort);
  await rt.session.expectNoFault('the boot path of an absent machine');
}

/**
 * `RESTART` once the session is stable, then the wait for the new boot in emulated time. Two boots in a row that end before the stable period (30 s of
 * DSP blocks, `DkrtBootInfo.stable`) start the third in safe mode, which loads no module (runtime/core/src/rs/src/safe_mode.rs `ui_pass`).
 */
async function restart(rt: TestRuntime, plan: RuntimePlan): Promise<void> {
  await pollUntil(rt.clock, async () => (await bootInfo(rt.session, plan)).stable, (v) => v === 1, {
    withinMs: 40_000,
    message: 'the session is stable (the boot counter is back to 0) before the restart',
  });
  await rt.dk.restart();
  await rt.dk.waitForBoot();
}

runtimeTests('an unloaded machine reads MISSING and plays silence, and the load restores the sound only', expectMissingAndSilent, { dsp: 'sharc' });

import path from 'node:path';
import { waitClock, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { captureNote, frames, pickSine, readFloatWav, word } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { TRACK_OBJ_WORD, decodeTables } from '@digi/regression/runtime/project-kit.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * An absent machine module reads MISSING and plays silence (docs/plans/extension-model.md E5d), on both devices. The machine is `test/sine` (the machine the other live tests use;
 * the plan names the pluck machine, whose pitch report needs the sample pool: the sine has the same row lifecycle).
 *
 * `UNLOAD` of the machine with the module outside the saved selection:
 * - the machine row's short name (the text the track label draws) is `MISS`, and the machine is not in the selection list (the rows with a list name, `decodeTables`);
 * - a note on the track renders digital silence: the peak over the held note is below 1e-4 (the placeholder voice row: DT2 through the disabled word, DN2 through the row's own number
 *   that no engine has), against the audible note before the unload;
 * - `LOAD` of it again: the short name is the machine's own, the note plays at the level it had (at least half), and the words of the track object (the kit's stored bytes, DT2: the
 *   track object `TRACK_OBJ_WORD`) are the words before the unload, the type byte included: the unload and the load wrote nothing to the track.
 *
 * Replaces the "a plain unload leaves a gap row that plays the first stock machine" assertions of `live.device.test.ts`.
 *
 * Written, not run (docs/plans/extension-model.md, "Device proof").
 */
const MIDI_CHANNEL = 1;
const AMP_CC = { dt2: { hold: 80, decay: 81 }, dn2: { hold: 85, decay: 86 } } as const;
const SEND_CC = { dt2: [84, 85, 12], dn2: [30, 31, 29] } as const;
/** Where the stored bytes of track 1 can be read: only the Digitakt II track object is mapped in the tests. */
const TRACK_OBJECT: Partial<Record<Device, { at: number; words: number }>> = { dt2: { at: TRACK_OBJ_WORD, words: 0x450 / 4 } };
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

/** The short name of the machine row of `type` (the row's word at +60: what the page and the track label draw). */
async function shortName(rt: TestRuntime, plan: RuntimePlan, type: number): Promise<string> {
  const [rows, first, size] = ['machines.rows', 'machines.first', 'machines.row_size'].map((s) => planSymbol(plan, s)) as [number, number, number];
  return cstr(rt, await word(rt.session, rows + size * (type - first) + 60));
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
  const where = TRACK_OBJECT[device];
  if (!where) return undefined;
  return Promise.all(Array.from({ length: where.words }, (_, i) => word(rt.session, where.at + 4 * i)));
}

async function expectMissingAndSilent(rt: TestRuntime): Promise<void> {
  const device = rt.session.options.device as Device;
  const artifacts = liveArtifacts(rt.session, 'missing');
  const sinePath = moduleStorePath(device as 'dt2' | 'dn2', 'test/sine');
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const plan = await prepareRuntimePlan(rt.session, artifacts, [
    'machines.rows',
    'machines.count',
    'machines.first',
    'machines.row_size',
    'pf_table',
    'params.first',
    'params.count',
  ]);
  // the next boot would load the debug module alone: the machine is outside the saved selection
  await rt.dk.select([moduleStorePath(device as 'dt2' | 'dn2', DEBUG_MODULE)]);
  const sine = (await rt.dk.modules()).find((m) => m.name === 'test/sine')!;
  const type = sine.types[0]!;
  await pickSine(rt);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device as 'dt2' | 'dn2'].hold, 127);
  await sendCc(rt, MIDI_CHANNEL, AMP_CC[device as 'dt2' | 'dn2'].decay, 64);
  for (const cc of SEND_CC[device as 'dt2' | 'dn2']) await sendCc(rt, MIDI_CHANNEL, cc, 0);

  const heard = await notePeak(rt, artifacts, 'before');
  expect(heard, `the machine is audible (peak ${heard.toFixed(4)})`).toBeGreaterThan(0.01);
  const ownShort = await shortName(rt, plan, type);
  expect(ownShort, 'the loaded machine has its own short name').not.toBe('MISS');
  const stored = await trackWords(rt, device);

  const gone = await rt.dk.unloadAndWait(sinePath);
  expect(gone, 'the unload').toEqual({ result: 0, unloaded: true });
  expect(await shortName(rt, plan, type), 'the track label of the absent machine').toBe('MISS');
  expect(Object.keys((await decodeTables(rt.session, plan)).machines).map((n) => n.toLowerCase()), 'the absent machine is not in the selection list').not.toContain('sine');
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
  if (stored) expect(after, 'the track object holds the words it held before the unload (the load restores the sound only)').toEqual(stored);
}

runtimeTests('an unloaded machine reads MISSING and plays silence, and the load restores the sound only', expectMissingAndSilent, { dsp: 'sharc' });

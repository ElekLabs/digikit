import path from 'node:path';
import { pollUntil, waitClock } from '@digi/test-support';
import { expect, test } from 'vitest';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { cardRig, RT, CARD_SYMBOLS } from '@digi/regression/runtime/card-rig.ts';
import { FIRMWARE, describeOn } from '@digi/regression/runtime/devices.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { moduleStorePath } from '@digi/regression/runtime/install.ts';
import { captureNote } from '@digi/regression/runtime/note-capture.ts';
import { frames } from '@digi/regression/runtime/dsp-state.ts';
import { readFloatWav } from '@digi/regression/runtime/pcm-files.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { sendCc, sendNote, sendNoteOff } from '@digi/regression/runtime/midi.ts';
import { buildPanelProject, dynType, loadPanelProject, savePanelProject, trackType } from '@digi/regression/runtime/project-kit.ts';
import type { TestRuntime } from '@digi/regression/support.ts';

/**
 * The project round trip of a reloaded machine (docs/plans/extension-model.md E5b item 6), Digitakt II: a project that uses `test/sine` on track 1 (two parameter values, a trig with a
 * p-lock and a sound lock, built on the panel as the project round trip tests do) is saved; the machine is reloaded with `RELOAD`; the project is loaded; track 1 holds the machine type again and a note
 * on track 1 plays the same sound as before the save: the pitch within 1% and the peak within 5% (the reload keeps the machine's type number, so the project's reference to it is still valid).
 * AMP HOLD is NOTE and the FX sends are 0, so a capture holds the machine alone.
 *
 * Written, not run (docs/plans/extension-model.md, "Device proof"). The Digitone II half is not written: `trackType` reads a Digitakt II track object.
 */
const ID = 'test/sine';
/** The store path of the module on the card (`modules/<kind>/<author>-<name>.dkm`). */
const PATH = moduleStorePath('dt2', ID);
const NOTE = 57;
const MIDI_CHANNEL = 1;
const CAPTURE_BLOCKS = 4000;

/** Upward zero crossings of the left channel of interleaved stereo `xs[from..to)`: the frequency between the first and the last. */
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
  async function note(rt: TestRuntime, artifacts: string, tag: string): Promise<{ hz: number; peak: number }> {
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
    const from = Math.floor(250 * perBlock) & ~1;
    const to = Math.floor(700 * perBlock);
    let peak = 0;
    for (let i = from; i < to; i++) peak = Math.max(peak, Math.abs(pcm[i]!));
    return { hz: leftHz(pcm, from, to), peak };
  }

  test('save a project with the machine, reload the machine, load the project: the same sound', async () => {
    const rt = await run.load('project', { modules: [ID], selection: [PATH] });
    const artifacts = liveArtifacts(rt.session, 'reload-project');
    const type = await dynType(rt.session, run.plan, ID, 'sine');
    await buildPanelProject(rt, [{ track: 1, type, hint: 7 }]);
    expect(await trackType(rt.session, 1), 'track 1 holds the machine').toBe(type);
    await savePanelProject(rt);
    // the capture holds the machine alone: AMP HOLD is NOTE, the stock FX sends are 0 (the CCs of the Digitakt II manual, B.5 and B.7)
    await sendCc(rt, MIDI_CHANNEL, 80, 127);
    await sendCc(rt, MIDI_CHANNEL, 81, 64);
    for (const cc of [84, 85, 12]) await sendCc(rt, MIDI_CHANNEL, cc, 0);
    const before = await note(rt, artifacts, 'before');
    expect(before.peak, `the machine is audible (peak ${before.peak.toFixed(4)})`).toBeGreaterThan(0.01);

    // queue the reload over the paused protocol, then let emulated time run until the job finishes: the job needs the DSP, and the paused protocol's
    // short steps under its 30 s wall deadline do not reach the end of a reload with a DSP load
    // DkrtBootInfo.reloads (offset 252) moves when a live job finished; with phase 2 it tells this job's end from the state an earlier job left
    const reloads = planSymbol(run.plan, 'DKRT_BOOT') + 252;
    const reloads0 = await word(rt.session, reloads);
    await run.usb(rt, (dk) => dk.reload(PATH));
    const end = await pollUntil(rt.clock, async () => ({ st: await run.usb(rt, (dk) => dk.reloadState()), n: await word(rt.session, reloads) }), (v) => v.st.phase === 2 && v.n > reloads0, { withinMs: 20_000, message: 'the reload of the machine finished' });
    expect({ result: end.st.result, unloaded: end.st.unloaded }, 'the reload of the machine').toEqual({ result: 0, unloaded: true });
    await loadPanelProject(rt);
    expect(await trackType(rt.session, 1), 'after the project load track 1 holds the type of the reloaded machine').toBe(type);
    const after = await note(rt, artifacts, 'after');
    record(artifacts, 'project-roundtrip', { before, after });
    expect(Math.abs(after.hz / before.hz - 1), `the pitch after the reload and the project load (${after.hz.toFixed(2)} Hz) is the pitch before (${before.hz.toFixed(2)} Hz)`).toBeLessThan(0.01);
    expect(Math.abs(after.peak / before.peak - 1), `the level after (${after.peak.toFixed(4)}) is the level before (${before.peak.toFixed(4)})`).toBeLessThan(0.05);
    await rt.session.expectNoFault('project round trip of a reloaded machine');
  }, 900_000);
});

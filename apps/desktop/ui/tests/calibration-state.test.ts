// The Calibration state the Profile pane owns (src/calibration-state.ts): a run does not depend on what the pane draws, so a target restart in the
// middle of it (the pane's reading resets) neither aborts it nor loses its captures; only disposing the pane stops it.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createRoot, onCleanup } from 'solid-js';
import type { RunOptions, RunOutcome } from '@digi/workbench-core/profile/calibration-run.ts';
import type { Calibrator } from '../src/calibration-backend.ts';
import { createCalibrationState } from '../src/calibration-state.ts';

const profile = { name: 'heavy', cases: [{ id: 'stock', modules: [] }], warmup_s: 1, capture_s: 2, repeats: 2 };
const capture = {
  caseId: 'stock',
  repeat: 1,
  file: 'stock-1.profile.json',
  profile: { schema: 'dk.profile/1', run: { id: 'emu-1', case_id: 'stock' }, dsp: { unit: 'insn' } },
};

function fake() {
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  const seen: { aborted: boolean; opts?: RunOptions } = { aborted: false };
  const calibrator: Calibrator = {
    profiles: () => [{ name: 'heavy', description: '', seconds: 6, profile: profile as never }],
    run: async (opts) => {
      seen.opts = opts;
      opts.onEvent?.({ kind: 'capture', case: 'stock', n: 1, of: 2 });
      opts.onCapture?.(capture as never);
      await Promise.race([gate, new Promise<void>((r) => opts.signal!.addEventListener('abort', () => r()))]);
      seen.aborted = !!opts.signal?.aborted;
      return {
        profile: 'heavy',
        emulator: true,
        captures: [capture as never],
        skipped: [],
        failed: [],
        stopped: seen.aborted,
      } as RunOutcome;
    },
    fit: async () => ({}),
  };
  return { calibrator, release, seen };
}

test('a target restart mid-run (reading reset, section unmounted) does not abort the run and its captures are kept', async () => {
  const f = fake();
  const running: boolean[] = [];
  await createRoot(async (dispose) => {
    const st = createCalibrationState({
      calibrator: f.calibrator,
      emulator: () => true,
      blocked: () => false,
      onRunning: (r) => running.push(r),
    });
    const done = st.run();
    assert.equal(st.running(), true);
    // what a restart does to the pane: the drawn section goes away and comes back; the state is not part of it
    await Promise.resolve();
    f.release();
    await done;
    assert.equal(f.seen.aborted, false);
    assert.equal(st.running(), false);
    assert.equal(st.sets().emu?.captures.length, 1);
    assert.deepEqual(running, [true, false]);
    dispose();
  });
});

test('the table fills with each capture, before the run returns', async () => {
  const f = fake();
  await createRoot(async (dispose) => {
    const st = createCalibrationState({ calibrator: f.calibrator, emulator: () => true, blocked: () => false });
    const done = st.run();
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(st.running(), true);
    assert.equal(st.sets().emu?.captures.length, 1);
    f.release();
    await done;
    dispose();
  });
});

test('disposing the pane ends a run; Run refuses while something else of the pane is busy', async () => {
  const f = fake();
  await createRoot(async (dispose) => {
    const st = createCalibrationState({ calibrator: f.calibrator, emulator: () => true, blocked: () => false });
    onCleanup(() => st.dispose()); // what the pane does
    const done = st.run();
    dispose();
    await done;
    assert.equal(f.seen.aborted, true);
  });
  const g = fake();
  await createRoot(async (dispose) => {
    const st = createCalibrationState({ calibrator: g.calibrator, emulator: () => true, blocked: () => true });
    await st.run();
    assert.equal(g.seen.opts, undefined);
    dispose();
  });
});

test('the note before Run names the tracks of the chosen profile on the target', async () => {
  const f = fake();
  f.calibrator.tracks = async () => [1];
  await createRoot(async (dispose) => {
    const st = createCalibrationState({ calibrator: f.calibrator, emulator: () => true, blocked: () => false });
    await new Promise((r) => setTimeout(r, 0));
    assert.equal(st.note(), 'Calibration uses track 1 and leaves it on the stock machine.');
    dispose();
  });
});

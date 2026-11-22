import { readFileSync } from 'node:fs';
import path from 'node:path';
import { Sample, prepareUpload } from '@digi/dk-session/dksample';
import { openSamples } from '@digi/dk-session/samples';
import { repoRoot } from '@digi/test-support';
import { expect } from 'vitest';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { installWithDebug, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { AUDIBLE, RATE, captureNote, onset, selectDynamicMachine, tone, trackApi, until, wav } from './shot-support.ts';

// DT2 1.17: unload and load of a module that uses the sample pool (docs/plans/extension-model.md E5c, the sample-pool case of the live proof). `digikit/shot` needs two services
// (machines and samples), and its pool table belongs to it: the unload takes down the machine row, its voice rows and the pool table in one job, and the load puts them back.
//
// 1. The sample is on the card, bound to pool entry 0 (ready), and a note sounds (the 440 Hz and 660 Hz tones of its two channels in the PCM).
// 2. With the key held and the voice sounding, `UNLOAD` takes the module down: the job ends with result 0, no fault is logged, the module is not in MODULES and the held voice
//    does not play any more (the machine reads MISSING and its parked voice row renders silence: no stock machine's sound, E5d).
// 3. `LOAD` puts the module back: the pool table is new and empty, the same sample is bound again and ends ready, and a new note sounds with the same tones.
// 4. The capacity counts after the cycle are those before it.
//
// What is a host test and is not asserted here: that the unloaded module is gone from MODULES and LOADS says UNLOADED, and the order of the take-down (`dkrt-lifecycle`
// `manager/tests/take_down.rs`, `reload.rs` `an_unload_checks_the_module_drains_tears_down_and_ends_not_loaded_with_the_services_told_once`, assembly
// `the_live_jobs_steps_unload_a_module_and_load_it_again_and_another_with_the_service_staying_up`). The services stay up across the cycle (RC2b), so the counts of step 4 hold
// with the machines and samples services in.
//
// Run only with the go of the owner (docs/plans/extension-model.md, "Device proof").
const FOLDER = 'shotlive';
const SECONDS = 4;

runtimeTests(
  'LOAD and UNLOAD of a sample-pool module: unloaded with the voice sounding, loaded again, the sample bound and played again',
  async (runtime) => {
    const { session, dk } = runtime;
    const SHOT_PATH = moduleStorePath(session.options.device, 'digikit/shot');
    const artifacts = liveArtifacts(session, 'shot-live-dt2');
    const api = trackApi(runtime);
    await installWithDebug(runtime, 'digikit/shot', 'test/track-select', 'test/stock-pool');
    const start = await dk.capacity();
    await selectDynamicMachine(runtime, artifacts);

    // the sample: a different sine on each channel, so the tones in the PCM say that the sample plays
    const sample = await Sample.create(Uint8Array.from(readFileSync(path.join(repoRoot(), 'apps/web/public/dksample.wasm'))));
    const backend = openSamples('stock-rpc', { session: dk, hashOf: (data) => sample.hash(data) });
    await backend.mkdir(FOLDER);
    const file = prepareUpload(sample, 'a.wav', wav(SECONDS, [440, 660]));
    await backend.put(FOLDER, file);
    expect((await backend.verify(FOLDER, file)).ok, 'the sample verifies on the card').toBe(true);
    const bind = async (why: string) => {
      expect(await api.select(`/${FOLDER}/a`, 0), `${why}: the sample takes pool entry 0`).toBe(0);
      const ready = await api.settle(0);
      expect(ready.state, `${why}: the entry is ready: ${JSON.stringify(ready.seen)}`).toBe(3);
      return ready;
    };
    await bind('first');

    // 1 and 2: a note, and the unload while it sounds
    let ended: { result: number; unloaded: boolean } | undefined;
    let pressedBeforeUnload = 0;
    const held = await captureNote(runtime, path.join(artifacts, 'held.wav'), async (clock) => {
      await until(clock, (await clock()) + 0.6 * RATE);
      pressedBeforeUnload = await clock();
      ended = await dk.unloadAndWait(SHOT_PATH);
      await until(clock, (await clock()) + 1.0 * RATE);
    });
    expect(ended, 'the unload ended with the module torn down').toEqual({ result: 0, unloaded: true });
    const start0 = onset(held);
    expect(start0, 'the sample was audible before the unload').toBeGreaterThanOrEqual(0);
    const l = [440, 660].map((hz) => Math.max(tone(held.l, hz, start0 + 4800, pressedBeforeUnload), tone(held.r, hz, start0 + 4800, pressedBeforeUnload)));
    record(artifacts, 'held-tones', { before: l, unloadAt: pressedBeforeUnload });
    for (const t of l) expect(t, `the tones of the sample before the unload (${l.join(' ')})`).toBeGreaterThan(AUDIBLE);
    await session.expectNoFault('unload with the voice sounding');
    const after = [440, 660].map((hz) => Math.max(tone(held.l, hz, pressedBeforeUnload + RATE / 4, held.l.length), tone(held.r, hz, pressedBeforeUnload + RATE / 4, held.l.length)));
    for (const t of after) expect(t, `the sample does not play after the unload (${after.join(' ')})`).toBeLessThan(AUDIBLE / 4);
    // the absent machine plays silence, not the first stock machine's sound
    const quiet = [held.l, held.r].reduce((m, ch) => ch.subarray(Math.floor(pressedBeforeUnload + RATE / 4)).reduce((p, x) => Math.max(p, Math.abs(x)), m), 0);
    expect(quiet, `the held voice of the absent machine is silent (peak ${quiet.toExponential(2)})`).toBeLessThan(1e-4);

    // 3: load again, bind the sample again, play it
    expect(await dk.loadAndWait(SHOT_PATH), 'the load ended with the module in').toEqual({ result: 0, unloaded: false });
    await bind('after the load');
    const again = await captureNote(runtime, path.join(artifacts, 'again.wav'), async (clock) => {
      await until(clock, (await clock()) + 1.5 * RATE);
    });
    const on = onset(again);
    expect(on, 'the sample is audible after the load').toBeGreaterThanOrEqual(0);
    const back = [440, 660].map((hz) => Math.max(tone(again.l, hz, on + 4800, on + RATE), tone(again.r, hz, on + 4800, on + RATE)));
    for (const t of back) expect(t, `the tones of the sample after the load (${back.join(' ')})`).toBeGreaterThan(AUDIBLE);
    await session.expectNoFault('note after the load');

    // 4: the counts are those before the cycle
    const end = await dk.capacity();
    for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
      expect(end[kind]?.used, `${kind} in use after the cycle`).toBe(start[kind]?.used);
    }
    record(artifacts, 'live-samples', { before: l, after, again: back });
  },
  { dsp: 'sharc', devices: ['dt2'] },
);

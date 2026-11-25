import path from 'node:path';
import { button, emulatorClock, MS, repoRoot, waitClock } from '@digi/test-support';
import { expect, test } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { servicePath } from '@digi/regression/runtime/service-files.ts';
import { refusedCalls, serviceSymbol } from '@digi/regression/runtime/service-symbols.ts';

/**
 * The owner's two-phase call on the firmware (`Owner::step`): the fixture `test/local-step` takes the test hook of the SETTINGS list rebuild, and every rebuild is one step.
 * `decide` counts the step inside the owner; `act`, with the owner idle, calls the service's own export, which enters the owner again (a firmware function that calls back
 * into the service does the same), so the re-entries equal the steps and the owner refuses nothing. One nested call inside the owner, armed through a word of the file, is
 * refused and counted once, so a count of 0 means something. What the host shows (the counts over the fixture's entries, `provider_step_assembly_tests.rs`) is not repeated:
 * this test owns what only the firmware shows, that the hook of the rebuild runs in the UI task and finds the owner idle in `act`.
 */
const STATS = 'LOCAL_STEP_STATS';
const ARM = 2;

cardRigs('owner step', { dsp: 'zero', roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  test('a hook that runs Owner::step finds the owner idle in act; a nested call is the one that is refused', async () => {
    const debug = h.mod('digikit/debug', 'debug');
    const provider = { name: servicePath('local-step'), file: moduleArtifact(h.device, 'test/local-step') };
    // the test hook is listed by the port file of the test image (patchkit:ports-test), as in the providers test
    const leaf = `${h.device}-${FIRMWARE[h.device]}.dkport`;
    const port = { name: `ports/${leaf}`, file: path.join(repoRoot(), 'dist/ports-test', leaf) };
    const rt = await h.load('owner-step', { modules: [debug, provider, port], selection: [debug.name] });
    const { session } = rt as PatchedSession;
    const stats = await serviceSymbol(rt, h.plan, 'local-step', STATS);
    const read = async (index: number) =>
      ((await session.client.call('mem.cf.peek', { addr: stats + 4 * index })) as { value: number }).value >>> 0;
    const counts = async () => ({ steps: await read(0), reentries: await read(1), arm: await read(ARM) });

    // a few SETTINGS list rebuilds: each is one step, whose act called the export
    for (const key of ['SETTINGS', 'SETTINGS']) {
      await session.client.inputTap(button(key), 100 * MS);
      await waitClock(emulatorClock(session), 300); // the rebuild runs in the UI task: no event marks it
    }
    const before = await counts();
    expect(before.steps, 'the hook ran steps').toBeGreaterThanOrEqual(1);
    expect(before.reentries, 'each act re-entered the owner').toBe(before.steps);
    expect(await refusedCalls(rt, h.plan, 'local-step'), 'no call found the owner busy').toBe(0);

    // one nested call inside the owner: refused, counted once, and the step still completes
    await session.client.call('mem.cf.poke', { addr: stats + 4 * ARM, value: 1 });
    await session.client.inputTap(button('SETTINGS'), 100 * MS);
    await waitClock(emulatorClock(session), 300);
    const after = await counts();
    expect(after.arm, 'the step took the arming word').toBe(0);
    expect(after.steps, 'the armed event is a step').toBeGreaterThan(before.steps);
    expect(after.reentries, 'its act re-entered too').toBe(after.steps);
    expect(await refusedCalls(rt, h.plan, 'local-step'), 'the nested call was refused once').toBe(1);
    await session.expectNoFault('owner step');
  });
});

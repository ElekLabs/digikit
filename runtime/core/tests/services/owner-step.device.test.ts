import path from 'node:path';
import { repoRoot } from '@digi/test-support';
import { expect, test } from 'vitest';
import type { PatchedSession } from '@digi/regression/test-runtime.ts';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { moduleArtifact } from '@digi/regression/runtime/module-artifact.ts';
import { FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { servicePath } from '@digi/regression/runtime/service-files.ts';
import { refusedCalls, serviceSymbol } from '@digi/regression/runtime/service-symbols.ts';

/**
 * The owner's two-phase call on the firmware (`Owner::step`): the fixture `test/local-step` takes the test hook of the SETTINGS list build, which runs once per boot. The first
 * step enters the owner: `decide` counts it and makes one nested call inside the owner, which the owner refuses and counts; `act`, with the owner idle, calls the service's own
 * export, which enters the owner again (a firmware function that calls back into the service does the same). Firmware fact: an act outside the owner re-enters the service;
 * a nested call inside the owner is refused and counted. What the host shows (`provider_step_assembly_tests.rs`) is not repeated: this test owns that the hook runs in the UI
 * task and finds the owner idle in `act`.
 */
const STATS = 'LOCAL_STEP_STATS';

cardRigs('owner step', { dsp: 'zero', roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  test('a hook that runs Owner::step finds the owner idle in act; a nested call is the one that is refused', async () => {
    const debug = h.mod('digikit/debug', 'debug');
    // a provider loads only when a selected client imports it: the client imports local-step's export
    const client = h.mod('test/local-step-user', 'local-step-user');
    const provider = { name: servicePath('local-step'), file: moduleArtifact(h.device, 'test/local-step') };
    // the test hook is listed by the port file of the test image (patchkit:ports-test), as in the providers test
    const leaf = `${h.device}-${FIRMWARE[h.device]}.dkport`;
    const port = { name: `ports/${leaf}`, file: path.join(repoRoot(), 'dist/ports-test', leaf) };
    const rt = await h.load('owner-step', { modules: [debug, client, provider, port], selection: [debug.name, client.name] });
    const { session } = rt as PatchedSession;
    const stats = await serviceSymbol(rt, h.plan, 'local-step', STATS);
    const read = async (index: number) =>
      ((await session.client.call('mem.cf.peek', { addr: stats + 4 * index })) as { value: number }).value >>> 0;
    const counts = async () => ({ steps: await read(0), reentries: await read(1) });

    const seen = await counts();
    expect(seen.steps, 'the hook ran the step').toBe(1);
    // two re-entries: the client's init calls the export once, and the hook's act re-enters the owner outside it
    expect(seen.reentries, 'the client init call and the act re-entered the owner').toBe(2);
    expect(await refusedCalls(rt, h.plan, 'local-step'), 'the nested call inside the owner was refused once').toBe(1);
    await session.expectNoFault('owner step');
  });
});

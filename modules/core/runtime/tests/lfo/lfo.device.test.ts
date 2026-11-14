import fs from 'node:fs';
import { LOAD_STATUS } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { waitForBootEmulated } from '@digi/regression/runtime/waits.ts';

const A = 'test/lfo-shape';
const B = 'test/lfo-shape-dup';
const stored = (id: string) => `modules/${id.split('/').at(-1)}.dkm`;
const code = (name: string) => LOAD_STATUS.find((s) => s[1] === name)![0];

/** R7 proves that the registry is generic: the `lfo` service owns the record kind `lfo_shape`, and two fixtures emit it with `dk_record_add`. The first loads and shows in
 * MODULES and CAPACITY; unloading it restores both; the second has the same shape id, so the boot plan refuses it before it loads (a second owner). No SHARC is needed. */
async function expectLfoShapes({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'the registry proof runs without the DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  const device = session.options.device;
  const before = { modules: await dk.modules(), cap: await dk.capacity() };

  // one fixture: loaded, no machine type, a record block in the arena
  await installModule(dk, A, moduleArtifact(device, A));
  const loaded = await dk.modules();
  expect(
    loaded.map((m) => [m.name, m.types]),
    'MODULES lists the fixture and it owns no machine type',
  ).toEqual([[A, []]]);
  const cap = await dk.capacity();
  expect(cap.MODULES!.used, 'one module record').toBe(before.cap.MODULES!.used + 1);
  expect(cap.ARENA!.used, 'the module and its record block take arena').toBeGreaterThan(before.cap.ARENA!.used);
  expect(await dk.loadFailures(), 'nothing was refused').toEqual([]);

  // a second owner of shape 7: both selected, the first loads and the plan refuses the second before it runs
  await dk.put(stored(B), fs.readFileSync(moduleArtifact(device, B)));
  await dk.select([stored(A), stored(B)]);
  await dk.restart();
  await waitForBootEmulated(dk);
  expect(
    (await dk.modules()).map((m) => m.name),
    'only the first owner loaded',
  ).toEqual([A]);
  expect(await dk.loadFailures(), 'the second owner is refused with the plan status').toEqual([
    { name: stored(B).split('/').at(-1)!, result: code('E_PLAN_KEY') },
  ]);

  // unload: no selection, a restart: MODULES and CAPACITY are what they were before the first load
  await dk.select([]);
  await dk.restart();
  await waitForBootEmulated(dk);
  expect((await dk.modules()).map((m) => m.name)).toEqual(before.modules.map((m) => m.name));
  const after = await dk.capacity();
  for (const kind of ['MODULES', 'SELECTION', 'ARENA'] as const) {
    expect(after[kind]!.used, `${kind} is as before`).toBe(before.cap[kind]!.used);
  }
  await session.expectNoFault('lfo shapes');
}

runtimeTests('the lfo service owns lfo_shape: a fixture loads and unloads, a second owner is refused', expectLfoShapes, {
  dsp: 'zero',
});

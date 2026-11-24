import fs from 'node:fs';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { neededServices, portFile } from '@digi/regression/runtime/service-files.ts';

const A = 'test/lfo-shape';
/** The lfo service is a file of the card, `services/lfo.dks`: the runtime finds it by its name when a selected module needs the service. It has hooks, so the
 * file on the card is the packed one, with the port file of the firmware (`ports/<device>-<version>.dkport`, `patchkit:ports`). */
const SERVICE_PATH = 'services/lfo.dks';

/** What only the firmware shows: with the service file on the card the hooks install into the real image (the six interrupt hooks per device and the UI hooks, patched
 * in the lfo tick that vector 191 runs), a fixture that emits `lfo_shape` loads and shows in MODULES and CAPACITY, and unloading it restores both with no fault. The
 * refusals (second owner E_PLAN_KEY, another ABI E_SVC_ABI, a kind table init does not declare E_DECL_KIND, no service file E_MISSING) and the restore of the stock
 * bytes are assembly tests (`runtime/core/src/rs/src/package/loader/tests/assembly_tests.rs`). No SHARC is needed. */
async function expectLfoShapes({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'the registry proof runs without the DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  const device = session.options.device;
  const before = { modules: await dk.modules(), cap: await dk.capacity() };

  // the service file goes on the card; the fixtures that emit lfo_shape need it
  const [needed] = neededServices([moduleArtifact(device, A)]);
  expect(needed?.service, 'the fixture needs the lfo service').toBe('lfo');
  const service = fs.readFileSync(moduleArtifact(device, needed!.moduleId));
  const port = portFile(device);
  const portBytes = fs.readFileSync(port.file);
  await dk.put(port.name, portBytes);
  expect(await dk.verifyFile(port.name, portBytes), 'fresh card read matches the port file').toBe(true);
  await dk.put(SERVICE_PATH, service);
  expect(await dk.verifyFile(SERVICE_PATH, service), 'fresh card read matches the packed service file').toBe(true);

  // one fixture: loaded, no machine type, a record block in the arena
  await installModule(dk, A, moduleArtifact(device, A), { device });
  const loaded = await dk.modules();
  expect(
    loaded.map((m) => [m.name, m.types]),
    'MODULES lists the fixture and it owns no machine type',
  ).toEqual([[A, []]]);
  const cap = await dk.capacity();
  expect(cap.MODULES!.used, 'one module record').toBe(before.cap.MODULES!.used + 1);
  expect(cap.ARENA!.used, 'the module and its record block take arena').toBeGreaterThan(before.cap.ARENA!.used);
  expect(await dk.loadFailures(), 'nothing was refused').toEqual([]);

  // unload: no selection, a restart: MODULES and CAPACITY are what they were before the first load
  await dk.select([]);
  await dk.restart();
  await dk.waitForBoot();
  expect((await dk.modules()).map((m) => m.name)).toEqual(before.modules.map((m) => m.name));
  const after = await dk.capacity();
  for (const kind of ['MODULES', 'SELECTION', 'ARENA'] as const) {
    expect(after[kind]!.used, `${kind} is as before`).toBe(before.cap[kind]!.used);
  }

  await session.expectNoFault('lfo shapes');
}

runtimeTests('the lfo service file owns lfo_shape: a fixture loads and unloads on the firmware with the hooks installed', expectLfoShapes, {
  dsp: 'zero',
});

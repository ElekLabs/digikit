import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { LOAD_STATUS, LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { DEBUG_MODULE, installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { ofKind } from '@digi/regression/runtime/runtime-menu.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { entry, nameHash, patched } from '../refusal/dkm-patch.ts';

const A = 'test/lfo-shape';
/** The same file under another module name: the same shape key, so a second owner of it. */
const B = 'test/lfo-shape-copy';
/** The lfo service is a file of the card, `services/lfo.dks` (R12-4): the runtime finds it by its name when a selected module needs the service. It has no
 * hooks, so there is nothing to `patchkit bind` against the firmware: the packed file goes on the card as it is. */
const SERVICE = 'core/services/lfo';
const SERVICE_PATH = 'services/lfo.dks';
/** Header words of a `.dks` (packages/dkneeds/src/gen_limits.rs): the sha256 and the ABI fingerprint. */
const H_SHA = 0x10;
const H_SVC_ABI = 0x3c;
/** The tag of the KINDS entry of the header table. */
const TAG_KINDS = 4;
/** The `MOD_LOAD` index of a provider (`loader::PROVIDER_IDX`): args are [index, result, file name, service hash, base]. */
const PROVIDER_IDX = 0xfffffffe;
const stored = (id: string) => `modules/${id.split('/').at(-1)}.dkm`;
const code = (name: string) => LOAD_STATUS.find((s) => s[1] === name)![0];

/** A copy of the service file whose ABI word is `word`, with its sha256 made right again, so that only the ABI word is wrong. */
function withAbiWord(file: Buffer, word: number): Buffer {
  const f = Buffer.from(file);
  f.writeUInt32BE(word >>> 0, H_SVC_ABI); // an ABI word with the top bit set is negative after `^` in JavaScript
  f.fill(0, H_SHA, H_SHA + 32);
  createHash('sha256').update(f).digest().copy(f, H_SHA);
  return f;
}

/** R7 proves that the registry is generic: the `lfo` service owns the record kind `lfo_shape`, and a fixture emits it with `dk_record_add`. The first loads and shows in
 * MODULES and CAPACITY; unloading it restores both; the second is the same file under another name, so it has the same shape key and the boot plan refuses it before it loads (a second owner). No SHARC is needed.
 *
 * R12-4: the service is the file `services/lfo.dks`, not a row of the image. Two refusals follow from that: a file built for another ABI is refused with `E_SVC_ABI` (a log
 * event of the provider) and the fixture that needs the service is REFUSED with `E_MISSING`; with no file on the card the fixture is REFUSED with `E_MISSING` as well. */
async function expectLfoShapes({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'the registry proof runs without the DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  const device = session.options.device;
  const before = { modules: await dk.modules(), cap: await dk.capacity() };

  // the service file goes on the card; the fixtures that emit lfo_shape need it
  const service = fs.readFileSync(moduleArtifact(device, SERVICE));
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

  // a second owner of the shape key (the file of A under another name): both selected, the first loads and the plan refuses the second before it runs
  await dk.put(stored(B), patched(fs.readFileSync(moduleArtifact(device, A)), B));
  await dk.select([stored(A), stored(B)]);
  await dk.restart();
  await dk.waitForBoot();
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
  await dk.waitForBoot();
  expect((await dk.modules()).map((m) => m.name)).toEqual(before.modules.map((m) => m.name));
  const after = await dk.capacity();
  for (const kind of ['MODULES', 'SELECTION', 'ARENA'] as const) {
    expect(after[kind]!.used, `${kind} is as before`).toBe(before.cap[kind]!.used);
  }

  // a service file built for another ABI: the loader refuses the provider with E_SVC_ABI, so the service is missing and the fixture that needs it is REFUSED.
  // A provider has no record in MODULES or LOADS (R12-3): its result is the log event MOD_LOAD with the provider marker, so the debug module is selected too.
  const abi = service.readUInt32BE(H_SVC_ABI);
  expect(abi, 'the packed file carries the ABI word (0 would skip the check)').not.toBe(0);
  const leafA = stored(A).split('/').at(-1)!;
  const debug = `modules/${DEBUG_MODULE.split('/').at(-1)}.dkm`;
  await dk.put(debug, fs.readFileSync(moduleArtifact(device, DEBUG_MODULE)));
  const lfoHash = nameHash('lfo');
  const expectNotLoaded = async (why: string) => {
    await dk.select([debug, stored(A)]);
    await dk.restart();
    await dk.waitForBoot();
    expect(
      (await dk.modules()).map((m) => m.name),
      `${why}: the fixture did not load`,
    ).not.toContain(A);
    const states = await dk.loadStates();
    expect(
      states.filter((m) => m.name === leafA).map((m) => [m.state, m.result, m.detail[0]]),
      `${why}: the fixture is REFUSED with the hash of its service`,
    ).toEqual([['REFUSED', code('E_MISSING'), lfoHash]]);
    return ofKind(await dk.readLog(), LOG_KIND.MOD_LOAD)
      .filter((e) => e.args[0] === PROVIDER_IDX)
      .map((e) => ({ result: e.args[1]! | 0, hash: e.args[3]! }));
  };
  await dk.put(SERVICE_PATH, withAbiWord(service, abi ^ 1));
  expect(await expectNotLoaded('a service file of another ABI'), 'the provider is refused with E_SVC_ABI').toEqual([{ result: code('E_SVC_ABI'), hash: lfoHash }]);

  // a service file whose KINDS item names another kind than the one `init` declares: the commit finds the difference (E_DECL_KIND). A module cannot show this
  // refusal: the role check refuses a module's KINDS (E_FORMAT) before `init`. The item's kind word is the first word of the KINDS payload.
  const wrongKind = Buffer.from(service);
  wrongKind.writeUInt32BE(91, entry(wrongKind, TAG_KINDS).at);
  wrongKind.fill(0, H_SHA, H_SHA + 32);
  createHash('sha256').update(wrongKind).digest().copy(wrongKind, H_SHA);
  await dk.put(SERVICE_PATH, wrongKind);
  expect(
    (await expectNotLoaded('a service file with a kind table that init does not declare')).map((p) => p.result),
    'the provider is refused with E_DECL_KIND',
  ).toEqual([code('E_DECL_KIND')]);

  // no service file on the card: the same fixture is E_MISSING, and no provider ran
  await dk.delete(SERVICE_PATH);
  expect(await expectNotLoaded('no service file'), 'no provider event').toEqual([]);

  await dk.select([]);
  await dk.restart();
  await dk.waitForBoot();
  await session.expectNoFault('lfo shapes');
}

runtimeTests('the lfo service file owns lfo_shape: a fixture loads and unloads, a second owner, another ABI, a kind table that init does not declare and no file are refused', expectLfoShapes, {
  dsp: 'zero',
});

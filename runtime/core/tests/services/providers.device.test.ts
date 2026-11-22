import fs from 'node:fs';
import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { button, emulatorClock, MS, repoRoot, waitClock } from '@digi/test-support';
import { expect, test } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { CARD_SYMBOLS, cardRigs, RT, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { derivedModule } from '@digi/regression/runtime/service-files.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { ofKind } from '@digi/regression/runtime/runtime-menu.ts';
import { entry, nameHash, patched, rehash } from '../refusal/dkm-patch.ts';

/**
 * Providers (docs/plans/runtime-and-services.md section 13.2, R12-3; docs/plans/port-file.md): the selection [debug, svc-test-user] needs the service `svc-test`, which
 * is not the runtime's. The service names the firmware, so it needs the port file: the boot finds `ports/<device>-<version>.dkport` (built from committed data by
 * `patchkit:ports-test`, no firmware bytes in it) by its firmware identity, loads it first, then `services/svc-test.dks`, which takes the test hook `cf.menu.hook.settings_list.test`
 * (the last step of the SETTINGS list rebuild) after its `init`. The user calls `dk_svctest_ping` (42) and logs it, and logs how often the hook ran. The provider file is
 * the packed one. The image is `rt`: no core/ui and no core/digikit-menu, which hook the same site.
 *
 * The log is the evidence: a provider has no module record, so MODULES and LOADS do not list it. Its result is the `MOD_LOAD` event with the provider index
 * `0xfffffffe`: [index, result, first four characters of the file name, service hash, base]. The tests need `digikit/debug` first in the selection.
 */
const PROVIDER_IDX = 0xfffffffe;
const E_PLAN_OWNER = loadCode('E_PLAN_OWNER');
const E_HOOK_STOCK = loadCode('E_HOOK_STOCK');
const E_MISSING = loadCode('E_MISSING');
/** The hook the fixture takes, and what the stock site holds (the displaced `movea.l (a2),a0` and `pea (2).w`). */
const HOOK = 'cf.menu.hook.settings_list.test';
const STOCK_SITE = [0x2052, 0x4878, 0x0002];
/** The tags of the header table. */
const TAG_NEEDS = 1;
const TAG_HOOKS = 6;
const TAG_PROVIDES = 9;
const TAG_SITES = 13;
/** Bytes of a SITES item (hash, address, length, kind, flags, check value) and the offset of its check value. */
const SITE_BYTES = 20;
const SITE_CHECK = 12;

const text = (words: number[]) =>
  String.fromCharCode(...words.flatMap((w) => [w >>> 24, (w >>> 16) & 255, (w >>> 8) & 255, w & 255]).filter(Boolean));

interface Bound {
  /** The packed service file (it holds no firmware bytes). */
  file: string;
  /** The port file of the test image for the firmware of the device (`patchkit:ports-test`). */
  port: string;
  /** The site of the hook on this firmware, from the port file. */
  site: number;
  /** Where in the port file the SITES item of the hook starts (an offset in the file). */
  siteItem: number;
  /** The image offset of the entry the hook jumps to, from the HOOKS item of the service file. */
  entryOff: number;
}

/** The packed `test/svc-test`, the port file of the device's firmware and the facts the test needs from them. */
function bind(rig: CardRig): Bound {
  const file = moduleArtifact(rig.device, 'test/svc-test');
  // the port file of the test image: it lists the fixture's hook, which a production port file does not (the site is digikit-menu's too)
  const port = path.join(repoRoot(), 'dist/ports-test', `${rig.device}-${FIRMWARE[rig.device]}.dkport`);
  const svc = fs.readFileSync(file);
  const hooks = entry(svc, TAG_HOOKS);
  expect(hooks.count, 'the service takes its one hook').toBe(1);
  expect(svc.readUInt32BE(hooks.at), 'the hook of the service').toBe(nameHash(HOOK));
  // the top byte of the entry is the device mask of the packer: the loader drops it
  const entryOff = svc.readUInt32BE(hooks.at + 4) & 0xffffff;
  const pf = fs.readFileSync(port);
  const sites = entry(pf, TAG_SITES);
  let item = -1;
  for (let k = 0; k < sites.count; k++) if (pf.readUInt32BE(sites.at + SITE_BYTES * k) === nameHash(HOOK)) item = sites.at + SITE_BYTES * k;
  expect(item, 'the port file lists the hook').toBeGreaterThanOrEqual(0);
  return { file, port, site: pf.readUInt32BE(item + 4), siteItem: item, entryOff };
}

/** The first six bytes at `site` as three words. */
async function siteBytes({ session }: PatchedSession, site: number): Promise<number[]> {
  const [a, b] = await Promise.all([session.client.memPeek(site), session.client.memPeek(site + 4)]);
  if (a === null || b === null) throw new Error(`hook site ${site.toString(16)} is not mapped`);
  return [a >>> 16, a & 0xffff, b >>> 16];
}

type Log = Parameters<typeof ofKind>[0];

/** The provider events of the boot log, in order: result, service hash and base. */
const providers = (log: Log) =>
  ofKind(log, LOG_KIND.MOD_LOAD)
    .filter((e) => e.args[0] === PROVIDER_IDX)
    .map((e) => ({ seq: e.seq, result: e.args[1]! | 0, name: text([e.args[2]!]), hash: e.args[3]!, base: e.args[4]! }));

/** The values a module logged under `message` (`dk_log`: the text in four words, then the value). */
const logged = (log: Log, message: string) =>
  ofKind(log, LOG_KIND.MOD_LOG)
    .filter((e) => text(e.args.slice(0, 4)).startsWith(message))
    .map((e) => e.args[4]!);

/** The events of the provider with the service hash `hash`. */
const providerOf = (log: Log, hash: number) => providers(log).filter((p) => p.hash === hash);

/** A folder under dist/ for the files the tests derive. */
function scratch(device: string): string {
  const dir = path.join(repoRoot(), 'dist/svc-providers', device);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

cardRigs('service providers', { dsp: 'zero', roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  const modules = () => ({ debug: h.mod('digikit/debug', 'debug'), user: h.mod('test/svc-test-user', 'user') });

  test('a selected client calls a service declared outside the SDK', async () => {
    const { debug } = modules();
    const client = h.mod('test/local-ping-user', 'local-ping-user');
    const provider = { name: 'services/local-ping.dks', file: moduleArtifact(h.device, 'test/local-ping') };
    const rt = await h.load('local-ping', { modules: [debug, client, provider], selection: [debug.name, client.name] });
    const log = await h.usb(rt, (dk) => dk.readLog());
    expect(providers(log).map((p) => [p.hash, p.result])).toEqual([[nameHash('local-ping'), 0]]);
    expect(logged(log, 'local-ping')).toEqual([42]);
  });

  test('a provider installs its hook and the consumer calls it; a safe boot leaves the site stock', async () => {
    const bound = bind(h);
    const { debug, user } = modules();
    const provider = { name: 'services/svc-test.dks', file: bound.file };
    const port = { name: `ports/${h.device}-${FIRMWARE[h.device]}.dkport`, file: bound.port };
    const selection = [debug.name, user.name];
    const rt = await h.load('providers', { modules: [debug, user, provider, port], selection });
    const log = await h.usb(rt, (dk) => dk.readLog());

    // order: the port file loads first (OK, its tail copies in the arena), then the service that needs it, then the user
    const prov = providers(log);
    expect(prov, 'the port file and the service').toHaveLength(2);
    const [portLoad, svcLoad] = [providerOf(log, nameHash('port'))[0]!, providerOf(log, nameHash('svc-test'))[0]!];
    expect([portLoad.result, svcLoad.result, svcLoad.name], 'both providers loaded').toEqual([0, 0, 'svc-']);
    expect([portLoad.base, svcLoad.base], 'in the arena').not.toContain(0);
    expect(portLoad.seq, 'the port file before the service').toBeLessThan(svcLoad.seq);
    const userLoad = ofKind(log, LOG_KIND.MOD_LOAD).find((e) => e.args[0] === 1);
    expect(userLoad?.args[1], 'the user loaded').toBe(0);
    expect(svcLoad.seq, 'the provider before the user').toBeLessThan(userLoad!.seq);
    const states = await h.usb(rt, (dk) => dk.loadStates());
    expect(states.map((m) => m.state), 'debug and the user are ACTIVE (a provider is no module of the list)').toEqual(['ACTIVE', 'ACTIVE']);

    // the site holds `jmp.l` to the entry of the service (its base and the entry offset of the HOOKS item)
    const entryAt = svcLoad.base + bound.entryOff;
    expect(await siteBytes(rt, bound.site), 'the site jumps to the entry of the service').toEqual([0x4ef9, entryAt >>> 16, entryAt & 0xffff]);

    // the user's call of dk_svctest_ping reached the provider: 42 is in the log
    expect(logged(log, 'svctest-ping'), 'dk_svctest_ping() returned 42').toEqual([42]);

    // the firmware builds the SETTINGS list at start, so the hook ran already; the tap runs it again
    const hits = async () => logged(await h.usb(rt, (dk) => dk.readLog()), 'svctest-hits');
    await waitClock(emulatorClock(rt.session), 1500); // the hook's log line: no event marks it
    const before = await hits();
    expect(before.length, 'the user logged the count').toBeGreaterThan(0);
    expect(before.at(-1)!, 'the hook ran').toBeGreaterThanOrEqual(1);
    await rt.session.client.inputTap(button('SETTINGS'), 100 * MS);
    await waitClock(emulatorClock(rt.session), 3000); // the SETTINGS rebuild after the tap: no event marks it
    const after = await hits();
    expect(after.at(-1)!, 'the count does not fall').toBeGreaterThanOrEqual(before.at(-1)!);
    expect(after.at(-1)!, 'the count is a count').toBeLessThan(1000);
    await rt.session.expectNoFault('provider hook');

    // a safe boot (the safe-once flag of the selection record) loads no provider and no module, and leaves the site stock
    const safe = await h.boot(await h.card({ modules: [debug, user, provider, port], selection, flags: 1 }));
    const info = await h.ready(true);
    expect(info, 'safe mode by the flag').toMatchObject({ safe: 1, loaded: 0 });
    await safe.session.client.call('run.mode', { mode: 'paused' });
    expect(await siteBytes(safe, bound.site), 'safe mode leaves the site stock').toEqual(STOCK_SITE);
  });

  test('a port file whose check value differs is E_HOOK_STOCK and the service that needs it and its dependent are refused', async () => {
    const bound = bind(h);
    const f = Buffer.from(fs.readFileSync(bound.port));
    // one byte of the check value of the hook's SITES item: the live site bytes no longer hash to it
    f[bound.siteItem + SITE_CHECK]! ^= 1;
    const tampered = path.join(scratch(h.device), 'tampered.dkport');
    fs.writeFileSync(tampered, rehash(f));
    const { debug, user } = modules();
    const rt = await h.load('hook-stock', {
      modules: [debug, user, { name: 'services/svc-test.dks', file: bound.file }, { name: 'ports/tampered.dkport', file: tampered }],
      selection: [debug.name, user.name],
    });
    const log = await h.usb(rt, (dk) => dk.readLog());
    // the port file is refused before anything is placed; the service has no port file to take its site from
    expect(providerOf(log, nameHash('port')).map((p) => [p.result, p.base]), 'the port file is refused before it is placed').toEqual([[E_HOOK_STOCK, 0]]);
    // changed with the new boot (RC2b): the service needs the port file, so the manager refuses it for the refused need without placing it (E_MISSING, its own hash); before,
    // the loader placed it and its hook import failed (E_IMPORT, naming the hook). The user's refusal below is the same.
    expect(providerOf(log, nameHash('svc-test')).map((p) => [p.result, p.base]), 'the service is refused for the port file it needs, and not placed').toEqual([[E_MISSING, 0]]);
    expect(await siteBytes(rt, bound.site), 'the site was never written').toEqual(STOCK_SITE);
    const states = await h.usb(rt, (dk) => dk.loadStates());
    expect([states[1]!.state, states[1]!.result, states[1]!.detail], 'the user is REFUSED, with the hash of its service').toEqual([
      'REFUSED',
      E_MISSING,
      [nameHash('svc-test'), 0],
    ]);
    expect(logged(log, 'svctest-ping'), 'the user never ran').toEqual([]);
  });

  test('two providers of one hook site are E_PLAN_OWNER: the second is refused and its dependent is REFUSED', async () => {
    const bound = bind(h);
    const { debug, user } = modules();
    // a second provider, `svc-test-b`: the packed file with another service name in PROVIDES, so it takes the hook site that the first owns
    const b = Buffer.from(fs.readFileSync(bound.file));
    b.writeUInt32BE(nameHash('svc-test-b'), entry(b, TAG_PROVIDES).at);
    const second = path.join(scratch(h.device), 'svc-test-b.dks');
    fs.writeFileSync(second, rehash(b));
    // and a second user that needs it: the user file with its need of svc-test changed to svc-test-b (it never loads)
    const userB = patched(fs.readFileSync(moduleArtifact(h.device, 'test/svc-test-user')), 'svc-test-user-b', (f) => {
      const { at, count } = entry(f, TAG_NEEDS);
      for (let k = 0; k < count; k++) if (f.readUInt32BE(at + 12 * k) === nameHash('svc-test')) f.writeUInt32BE(nameHash('svc-test-b'), at + 12 * k);
    });
    const userBFile = path.join(scratch(h.device), 'svc-test-user-b.dkm');
    fs.writeFileSync(userBFile, userB);
    derivedModule(moduleArtifact(h.device, 'test/svc-test-user'), userBFile);
    const rt = await h.load('plan-owner', {
      modules: [
        debug,
        user,
        { name: 'modules/userb.dkm', file: userBFile },
        { name: 'services/svc-test.dks', file: bound.file },
        { name: 'services/svc-test-b.dks', file: second },
        { name: `ports/${h.device}-${FIRMWARE[h.device]}.dkport`, file: bound.port },
      ],
      selection: [debug.name, user.name, 'modules/userb.dkm'],
    });
    const log = await h.usb(rt, (dk) => dk.readLog());
    const prov = providers(log).map((p) => [p.hash, p.result]);
    expect(prov, 'the port file and the first provider load, the second takes a site that the first owns').toHaveLength(3);
    expect(prov).toContainEqual([nameHash('port'), 0]);
    expect(prov).toContainEqual([nameHash('svc-test'), 0]);
    expect(prov).toContainEqual([nameHash('svc-test-b'), E_PLAN_OWNER]);
    const states = await h.usb(rt, (dk) => dk.loadStates());
    expect(states[1]!.state, 'the user of the first provider loads').toBe('ACTIVE');
    expect([states[2]!.state, states[2]!.result, states[2]!.detail], 'the user of the second is REFUSED with its hash').toEqual([
      'REFUSED',
      E_MISSING,
      [nameHash('svc-test-b'), 0],
    ]);
    expect(logged(log, 'svctest-ping'), 'dk_svctest_ping() ran once, for the first user').toEqual([42]);
  });
});

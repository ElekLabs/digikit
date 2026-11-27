import fs from 'node:fs';
import path from 'node:path';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { button, emulatorClock, MS, pollUntil, repoRoot } from '@digi/test-support';
import { expect, test } from 'vitest';
import type { PatchedSession } from '@digi/regression/test-runtime.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { moduleArtifact } from '@digi/regression/runtime/module-artifact.ts';
import { packModuleVariant } from '@digi/regression/runtime/module-variant.ts';
import { ofKind } from '@digi/regression/runtime/runtime-menu.ts';
import { entry, nameHash } from '../refusal/dkm-patch.ts';

/**
 * Hot reload of a service (docs/plans, hot reload slice 1): `RELOAD services/svc-test.dks` takes `svc-test-user` and the service down, brings the service up from the new file and
 * the consumer back. The service is `test/svc-test` (it takes a hook site, `cf.menu.hook.settings_list.test`, and exports `dk_svctest_ping`); the second file is the same module
 * built again from a temporary copy with `return 42;` changed to `return 43;` (`packModuleVariant`), put on the card over the first.
 *
 * What only the firmware shows, one case:
 * - DF1: the hook site holds `jmp.l` to the entry of the provider before and after the reload (the installer restored the stock bytes under the mask and wrote the new jump), and the
 *   hook keeps running after it (the SETTINGS rebuild after a tap runs the new owner object's hook);
 * - DF2: the consumer reads the second version's value without a restart: its `init` logs 42 at the boot and 43 after the reload, and the runtime logs no fault.
 * The job's result and `unloaded` come from RELOAD_STATE; the wait is the job's own end on the session clock (no timer of ours). The order of the steps, the refusals and the boot record are
 * host tests (`core/runtime:test-assembly`, `reload/tests/service.rs`).
 */
const PROVIDER_IDX = 0xfffffffe;
const HOOK = 'cf.menu.hook.settings_list.test';
const STOCK_SITE = [0x2052, 0x4878, 0x0002];
const TAG_HOOKS = 6;
const TAG_SITES = 13;
const SITE_BYTES = 20;

const text = (words: number[]) =>
  String.fromCharCode(...words.flatMap((w) => [w >>> 24, (w >>> 16) & 255, (w >>> 8) & 255, w & 255]).filter(Boolean));
type Log = Parameters<typeof ofKind>[0];
const logged = (log: Log, message: string) =>
  ofKind(log, LOG_KIND.MOD_LOG)
    .filter((e) => text(e.args.slice(0, 4)).startsWith(message))
    .map((e) => ({ seq: e.seq, value: e.args[4]! }));

/** The first six bytes at `site` as three words. */
async function siteBytes({ session }: PatchedSession, site: number): Promise<number[]> {
  const [a, b] = await Promise.all([session.client.memPeek(site), session.client.memPeek(site + 4)]);
  if (a === null || b === null) throw new Error(`hook site ${site.toString(16)} is not mapped`);
  return [a >>> 16, a & 0xffff, b >>> 16];
}

cardRigs('service reload', { dsp: 'zero', roots: RT, symbols: CARD_SYMBOLS, uploads: true, relaxFs: true }, (h) => {
  test('RELOAD of a service file swaps the hook jump and the consumer reads the new export without a restart', async () => {
    const debug = h.mod('digikit/debug', 'debug');
    const user = h.mod('test/svc-test-user', 'user');
    const v1 = moduleArtifact(h.device, 'test/svc-test');
    const port = path.join(repoRoot(), 'dist/ports-test', `${h.device}-${FIRMWARE[h.device]}.dkport`);
    const out = path.join(repoRoot(), 'dist/svc-reload', h.device);
    const v2 = await packModuleVariant({
      moduleDir: 'tests/fixtures/modules/svc-test',
      id: 'test/svc-test',
      device: h.device as Parameters<typeof packModuleVariant>[0]['device'],
      edits: [{ file: 'src/c/svc_test.c', from: 'return 42;', to: 'return 43;' }],
      out: path.join(out, 'svc-test-v2.dks'),
    });
    // the site of the hook and the entry offset, from the port file and the service file
    const svc = fs.readFileSync(v1);
    const hooks = entry(svc, TAG_HOOKS);
    const entryOff = svc.readUInt32BE(hooks.at + 4) & 0xffffff;
    const pf = fs.readFileSync(port);
    const sites = entry(pf, TAG_SITES);
    let site = -1;
    for (let k = 0; k < sites.count; k++)
      if (pf.readUInt32BE(sites.at + SITE_BYTES * k) === nameHash(HOOK)) site = pf.readUInt32BE(sites.at + SITE_BYTES * k + 4);
    expect(site, 'the port file lists the hook').toBeGreaterThan(0);

    const provider = { name: 'services/svc-test.dks', file: v1 };
    const portFile = { name: `ports/${h.device}-${FIRMWARE[h.device]}.dkport`, file: port };
    const rt = await h.load('service-reload', { modules: [debug, user, provider, portFile], selection: [debug.name, user.name] });
    const before = await h.usb(rt, (dk) => dk.readLog());
    expect(logged(before, 'svctest-ping').map((e) => e.value), 'the consumer read the first version').toEqual([42]);
    const base = ofKind(before, LOG_KIND.MOD_LOAD).find((e) => e.args[0] === PROVIDER_IDX && e.args[3] === nameHash('svc-test'))!.args[4]!;
    const entryAt = base + entryOff;
    expect(await siteBytes(rt, site), 'DF1: the site jumps to the entry of the first copy').toEqual([0x4ef9, entryAt >>> 16, entryAt & 0xffff]);

    // the new file is on the card: the job takes the consumer and the service down and brings both back
    await h.usb(rt, (dk) => dk.put('services/svc-test.dks', fs.readFileSync(v2)));
    const mark = Math.max(...ofKind(before, LOG_KIND.MOD_LOG).map((e) => e.seq));
    const end = await h.usb(rt, (dk) => dk.reloadAndWait('services/svc-test.dks'));
    expect(end, 'the reload of the service ran to its end').toEqual({ result: 0, unloaded: true });

    const after = await h.usb(rt, (dk) => dk.readLog());
    const pings = logged(after, 'svctest-ping');
    expect(pings.map((e) => e.value), 'DF2: the consumer reads 43 after the reload, with no restart').toEqual([42, 43]);
    expect(pings[1]!.seq, 'the new value was logged after the reload began').toBeGreaterThan(mark);
    expect(ofKind(after, LOG_KIND.BOOT_START).length, 'no second boot').toBe(ofKind(before, LOG_KIND.BOOT_START).length);
    const jump = await siteBytes(rt, site);
    const target = ((jump[1]! << 16) | jump[2]!) >>> 0;
    expect([jump[0], target >= base - 0x100000 && target < base + 0x100000], 'DF1: the site holds jmp.l into the arena after the reload').toEqual([0x4ef9, true]);
    console.info(`DF1 ${h.device}: site ${site.toString(16)} jmp.l ${entryAt.toString(16)} before, ${target.toString(16)} after (same base: ${target === entryAt})`);
    expect(jump, 'the site is not stock').not.toEqual(STOCK_SITE);

    // the hook runs with the new owner object: a tap on SETTINGS rebuilds the list, the consumer logs the hook count
    await rt.session.client.inputTap(button('SETTINGS'), 100 * MS);
    // the consumer's own MOD_LOG event marks the rebuild: wait for it in emulated time
    const hits = await pollUntil(
      emulatorClock(rt.session),
      async () => logged(await h.usb(rt, (dk) => dk.readLog()), 'svctest-hits').filter((e) => e.seq > pings[1]!.seq),
      (v) => v.length > 0,
      { withinMs: 30_000, message: 'the hook of the new copy logs a hit after the SETTINGS tap' },
    );
    expect(hits.length, 'the new copy of the hook ran after the reload').toBeGreaterThan(0);
    await rt.session.expectNoFault('service reload');
  }, 900_000);
});

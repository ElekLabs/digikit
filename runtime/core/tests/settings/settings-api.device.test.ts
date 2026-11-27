import type { DkSession } from '@digi/dk-session';
import { describe, expect, test } from 'vitest';
import { decodeDescriptor, settingValueCodec } from '@digi/module-ui';
import { bootInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { statusOf } from '@digi/regression/runtime/upload-bytes.ts';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';
import { cardRig } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { cardStore } from '@digi/regression/runtime/device-data.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import {
  decodeSettings,
  E_SET_KEY,
  E_SET_NOKEY,
  settingsMap,
  settingsRecord,
} from '@digi/regression/runtime/project-kit.ts';
import { RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';

/**
 * The module settings API (api 1.9, <dk/platform/settings.h>). The fixtures test/settings-a and test/settings-b are one source: a service command queues a put, get
 * or remove request, `tick` runs it in the UI task, and `init` reads the key "boot". Keys are stored as `<module name>/<key>`.
 */

const A = 'test/settings-a';
const B = 'test/settings-b';
const CMD = { [A]: 0x38, [B]: 0x3a };
const PUT = 1;
const GET = 2;
const REMOVE = 3;
const DELAY_MS = 2000; // the runtime writes the record this long after the last change
const text = (s: string) => Buffer.from(s);
const hex = (s: string) => text(s).toString('hex');

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} module settings API`, () => {
    const s = cardStore(device);
    const kit = cardRig(device, 'settings-api', {
      dsp: 'zero',
      roots: RT,
      symbols: ['DKRT_MAILBOX', 'DKRT_CARD', ...RING_SYMBOLS],
      initial: () => ({ selection: [] }),
      storeExtra: [s.setA, s.setB],
    });
    const spec = { modules: ['test/settings-a', 'test/settings-b'], selection: ['test/settings-a', 'test/settings-b'].map((id) => kit.mod(id).name) };

    /** One request of a fixture module: queued by the service command, run by `tick`, then read back. */
    async function call(
      rt: TestRuntime,
      module: typeof A | typeof B,
      op: number,
      key: string,
      value = Buffer.alloc(0),
    ) {
      const usb = <T>(run: (dk: DkSession) => Promise<T>) => kit.usb(rt, run);
      const body = Buffer.concat([Buffer.from([op, key.length]), text(key), value]);
      expect((await usb((dk) => dk.request(CMD[module], body))).status, 'the request is queued').toBe(0);
      // tick runs about 120 times a second; right after a boot the first passes can take longer, so the answer is asked again (emulated time) until the request ran
      let b = Buffer.alloc(0);
      for (let tries = 0; tries < 10; tries++) {
        await rt.ui.wait({ ms: 200, why: SETTLE });
        b = Buffer.from((await usb((dk) => dk.request(CMD[module] + 1))).body);
        if (b[0] === 0) break;
      }
      expect(b[0], 'tick ran the request').toBe(0);
      const rc = b.readInt32BE(1);
      return { rc, initRc: b.readInt32BE(5), init: b.subarray(9, 25), val: b.subarray(25, 25 + Math.max(rc, 0)) };
    }
    const record = async (rt: TestRuntime) =>
      settingsMap(decodeSettings((await kit.usb(rt, settingsRecord)).raw));

    test('a value put through the API is on the card after a restart and is read in init', async () => {
      const rt = await kit.load('persist', spec);
      expect((await bootInfo(rt.session, kit.plan)).results, 'both modules load').toEqual([0, 0]);
      const first = await call(rt, A, GET, 'boot');
      expect([first.initRc, first.rc], 'nothing is saved yet: init and get answer E_SET_NOKEY').toEqual([
        E_SET_NOKEY,
        E_SET_NOKEY,
      ]);
      expect((await call(rt, A, PUT, 'boot', text('xyz'))).rc, 'put').toBe(0);
      expect((await call(rt, A, GET, 'boot')).val, 'get gives the value back').toEqual(text('xyz'));
      expect(await record(rt), 'the stored key has the module name in front').toEqual({ [`${A}/boot`]: hex('xyz') });
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      const reboot = await kit.restart(rt);
      const again = await call(reboot, A, GET, 'boot');
      expect(again.initRc, 'init read 3 bytes').toBe(3);
      expect(again.init.subarray(0, 3), 'the value of the earlier boot').toEqual(text('xyz'));
      expect(await record(reboot), 'the record is as it was').toEqual({ [`${A}/boot`]: hex('xyz') });
      kit.allowStoreWrites(await kit.storeWrites());
    });

    test('the same key in two modules gives two records', async () => {
      const rt = await kit.load('collide', spec);
      expect((await call(rt, A, PUT, 'k', text('A1'))).rc).toBe(0);
      expect((await call(rt, B, PUT, 'k', text('B2'))).rc).toBe(0);
      expect(await record(rt), 'one stored key per module name').toEqual({
        [`${A}/k`]: hex('A1'),
        [`${B}/k`]: hex('B2'),
      });
      expect([(await call(rt, A, GET, 'k')).val, (await call(rt, B, GET, 'k')).val]).toEqual([text('A1'), text('B2')]);
      expect((await call(rt, B, REMOVE, 'k')).rc, 'remove').toBe(0);
      expect(await record(rt), 'removing B leaves the key of A').toEqual({ [`${A}/k`]: hex('A1') });
      expect((await call(rt, B, GET, 'k')).rc).toBe(E_SET_NOKEY);
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      kit.allowStoreWrites(await kit.storeWrites());
    });

    test('a key that is too long, leaves its prefix or breaks the rules is refused and nothing is written', async () => {
      const rt = await kit.load('refused', spec);
      const bad = ['k'.repeat(40), `../${B}/k`, 'a/../b', '/other', 'has space', ''];
      for (const key of bad) {
        expect((await call(rt, A, PUT, key, text('x'))).rc, `put ${JSON.stringify(key)}`).toBe(E_SET_KEY);
        expect((await call(rt, A, GET, key)).rc, `get ${JSON.stringify(key)}`).toBe(E_SET_KEY);
        expect((await call(rt, A, REMOVE, key)).rc, `remove ${JSON.stringify(key)}`).toBe(E_SET_KEY);
      }
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      expect(
        (await kit.storeWrites()).filter((x) => x !== s.selA && x !== s.selB),
        'no store sector is written besides the selection record (boot counter)',
      ).toEqual([]);
      expect(
        await statusOf(() => kit.usb(rt, settingsRecord)),
        'the settings record is still empty (SETTINGS answers E_SET_NOKEY)',
      ).toBe(E_SET_NOKEY);
    });
    test('a typed integer put through the module UI session is accepted, read back, and an out-of-range one is refused', async () => {
      const rt = await kit.load('typed', spec);
      expect((await bootInfo(rt.session, kit.plan)).results, 'both modules load').toEqual([0, 0]);
      // the endpoint lives only inside one USB session, so the whole exchange runs in it
      const r = await kit.usb(rt, async (dk) => {
        const mod = (await dk.modules()).find((m) => m.name === A);
        const ui = dk.moduleSettings(mod!.index, mod!.uiInstance!, () => true);
        const schema = decodeDescriptor(await ui.describe());
        const field = schema.settings.findIndex((x) => x.id === 'gain');
        const setting = schema.settings[field]!;
        const before = await ui.get(field);
        // gain has two decimals: the display value 0.75 is the stored i32 75, big endian
        const stored = settingValueCodec.encode(setting, 0.75);
        const put = await ui.put(field, stored, before.revision);
        const got = await ui.get(field);
        // 101 is above the maximum 100 (the codec refuses it, so the bytes are built here): the firmware refuses it
        const refused = await ui.put(field, Uint8Array.of(0, 0, 0, 101), got.revision).then(
          () => undefined,
          (e: { status?: string }) => e.status,
        );
        const after = await ui.get(field);
        return { uiInstance: mod?.uiInstance, field, stored, before, put, got, refused, after };
      });
      expect(r.uiInstance, 'settings-a registered its public settings').toBeTruthy();
      expect(r.field, 'the descriptor lists gain').toBeGreaterThanOrEqual(0);
      expect([...r.stored], 'stored i32 BE').toEqual([0, 0, 0, 75]);
      expect(r.put.revision, 'the put makes a new revision').not.toBe(r.before.revision);
      expect([...(r.got.value ?? [])], 'get returns the stored value').toEqual([0, 0, 0, 75]);
      expect(r.got.revision, 'get returns the new revision').toBe(r.put.revision);
      expect(r.refused, 'the firmware refuses a value above the maximum').toBe('E_SET_VALUE');
      expect([r.after.revision, [...(r.after.value ?? [])]], 'a refused put changes nothing').toEqual([
        r.got.revision,
        [0, 0, 0, 75],
      ]);
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      kit.allowStoreWrites(await kit.storeWrites());
    });
  });
}

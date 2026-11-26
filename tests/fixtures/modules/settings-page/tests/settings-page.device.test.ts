import { emulatorClock, emulatorRead, pollUntil, readTopMenuItems, viewStackFacts, type Device } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { cardRig } from '@digi/regression/runtime/card-rig.ts';
import { readChoiceWindow } from '@digi/regression/runtime/choice-window.ts';
import { cardStore } from '@digi/regression/runtime/device-data.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';
import { REDRAW } from '@digi/regression/runtime/panel-waits.ts';
import { rootsOf } from '@digi/regression/runtime/image-roots.ts';
import { RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';

/**
 * The settings page of a module (fixture `test/settings-page`, <dk/settings/page.h>) in the firmware: it opens from the module's SETTINGS... row of
 * DIGIKIT > MODULES, the stock choice popup hands an enum pick to the page, and the saved value is read in `init` after a restart. The fixture's command 0x0f reports
 * the three values and the calls of its `changed` function. The rows, the saves and the refusals are host tests (runtime/platform/ui/host `settings_page`).
 */
const REPORT = 0x0f;
const MODULE = 'test/settings-page';

type Report = { loud: number; gain: number; mode: number; calls: number; field: number; value: number };

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} module settings page`, () => {
    const s = cardStore(device);
    const kit = cardRig(device, 'settings-page', {
      dsp: 'zero',
      roots: rootsOf('rt-ui'),
      symbols: ['DKRT_MAILBOX', 'DKRT_CARD', ...RING_SYMBOLS],
      initial: () => ({ selection: [] }),
      storeExtra: [s.setA, s.setB],
    });
    const spec = () => ({ modules: [MODULE], selection: [kit.mod(MODULE).name] });

    const report = async (rt: TestRuntime): Promise<Report> => {
      const reply = await kit.usb(rt, (dk) => dk.request(REPORT));
      expect(reply.status, 'the module registered its command in init').toBe(0);
      const b = Buffer.from(reply.body);
      expect(b.length).toBe(24);
      return {
        loud: b.readInt32BE(0),
        gain: b.readInt32BE(4),
        mode: b.readInt32BE(8),
        calls: b.readInt32BE(12),
        field: b.readInt32BE(16),
        value: b.readInt32BE(20),
      };
    };
    const press = (rt: TestRuntime, key: string, ms = 300) => rt.ui.tap(key, { until: { ms, why: REDRAW } });

    /** DIGIKIT > MODULES > the module's side panel > SETTINGS... (the module is the only row; the panel is UNLOAD, SETTINGS..., INFO). */
    async function openPage(rt: TestRuntime): Promise<void> {
      await openDigikitMenu(rt);
      for (let i = 0; i < 3; i++) await press(rt, 'DOWN');
      await press(rt, 'YES', 800);
      await press(rt, 'RIGHT');
      await press(rt, 'DOWN');
      await press(rt, 'YES', 800);
    }

    test('the page of the module opens from its SETTINGS... row and shows one row for each field', async () => {
      const rt = await kit.load('page', spec());
      expect(await report(rt), 'the defaults of the table').toMatchObject({ loud: 1, gain: 50, mode: 1, calls: 0 });
      await openPage(rt);
      const facts = viewStackFacts(rt.session.options.device as Device);
      if (!facts) throw new Error(`${device} has no [ui] facts`);
      expect(await readTopMenuItems(emulatorRead(rt.session), facts), 'the Menu object of the page holds the three fields').toBe(3);
      kit.allowStoreWrites(await kit.storeWrites());
    });

    test('an enum picked in the stock popup reaches the module and is read in init after a restart', async () => {
      const rt = await kit.load('enum', spec());
      await openPage(rt);
      // MODE is the third row; the popup opens on the stored option (SLOW, the default, index 1), FAST is one below
      await press(rt, 'DOWN');
      await press(rt, 'DOWN');
      await press(rt, 'YES');
      expect((await readChoiceWindow(rt.session))?.selected, 'the popup opens on the stored value').toBe(1);
      await press(rt, 'DOWN');
      await press(rt, 'YES');
      expect(await report(rt), 'changed ran once, after the pick').toMatchObject({ mode: 2, calls: 1, field: 2, value: 2 });
      // the runtime writes the settings record some time after the last change: wait for that write to reach the store, not for a time
      await pollUntil(emulatorClock(rt.session), () => kit.storeWrites(), (sectors) => sectors.length > 0, {
        withinMs: 30_000,
        intervalMs: 500,
        message: 'the settings record was written to the store',
      });
      const reboot = await kit.restart(rt);
      expect(await report(reboot), 'the value of the earlier boot, read in init; no edit in this boot').toMatchObject({
        loud: 1,
        gain: 50,
        mode: 2,
        calls: 0,
      });
      kit.allowStoreWrites(await kit.storeWrites());
    });
  });
}

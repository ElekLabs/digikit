import { describe, expect, test } from 'vitest';
import { cardInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { DEVICES } from '@digi/regression/runtime/devices.ts';
import { decodeSettings, sectorOf, settingsMailbox } from '@digi/regression/runtime/project-kit.ts';
import { byte, DELAY_MS, ROUTE, settingsCase, suiteName } from './settings-support.ts';

/**
 * The settings store, part 2 (see settings.device.test.ts): a blank card is claimed. Which of two hand-made copies the boot reads (a torn newer copy)
 * is a host test (`settings_tests.rs` of the runtime, `dkrt_sdk::settings` for the choice).
 */

for (const device of DEVICES) {
  describe(suiteName(device), () => {
    const { s, kit, written } = settingsCase(device);

    // The boot of a blank card writes its selection record (copy A), which marks the area ours: the settings write needs no claim of the header pair.
    test('blank: the boot marks the area ours with its record, so the first settings write needs no claim and writes copy A', async () => {
      const rt = await kit.load('blank', {});
      const { session } = rt;
      const ci = await cardInfo(session, kit.plan);
      await settingsMailbox(session, kit.plan).put(ROUTE, byte(1));
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      const w = await written();
      const ci2 = await cardInfo(session, kit.plan);
      expect([ci.stateName, ci2.stateName, ci2.claims]).toEqual(['ours', 'ours', 0]);
      expect(w, 'copy A only: no header pair was claimed').toEqual([s.setA]);
      const after = await kit.after('blank', session.cardPath!);
      expect(decodeSettings(sectorOf(after, s.setA))?.entries[0]?.[0], 'the key in copy A on the card').toBe(ROUTE);
    });
  });
}

import { describe, expect, test } from 'vitest';
import { cardInfo } from '@digi/regression/runtime/card-kit.ts';
import type { CardSpec } from '@digi/regression/runtime/card-rig.ts';
import { SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { DEVICES } from '@digi/regression/runtime/devices.ts';
import {
  decodeSettings,
  encodeSettings,
  sectorOf,
  settingsMap,
  settingsMailbox,
} from '@digi/regression/runtime/project-kit.ts';
import { ringEvents } from '@digi/regression/runtime/runtime-menu.ts';
import { byte, DELAY_MS, padded, ROUTE, settingsCase, suiteName, text, torn } from './settings-support.ts';

/** The settings store, part 2 (see settings.device.test.ts): a blank card is claimed, and which of two hand-made copies the boot reads. */

for (const device of DEVICES) {
  describe(suiteName(device), () => {
    const { s, kit, written, dumpJson } = settingsCase(device);

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
      const d = await dumpJson(await kit.after('blank', session.cardPath!));
      expect(d.store, 'ours with a record and no header pair: the dump finds no store').toBeNull();
      expect(d.settings.active).toBe('A');
      expect(d.settings.copies[0].entries[0].key, 'the key in copy A').toBe(ROUTE);
    });

    // cards with both copies set by hand
    const old = encodeSettings(4, [['k', text('old')]]);
    const newTorn = torn(encodeSettings(5, [['k', text('new')]]));
    const two = (a: Buffer, b: Buffer): CardSpec => ({
      selection: [],
      foreign: { [s.setA]: padded(a), [s.setB]: padded(b) },
    });
    const copies = [
      {
        name: 'torn',
        card: two(old, newTorn),
        what: 'copy A (B is torn)',
        load: [2, 0, 4],
        value: [3, 'old'],
        error: 1,
        next: 1,
        seq: 5,
      },
    ] as const;
    test.each(copies)(
      'copies/$name: the boot reads $what and the next write goes to the other copy',
      async (want) => {
        const rt = await kit.load(`copies-${want.name}`, want.card);
        const { session } = rt;
        const mb = settingsMailbox(session, kit.plan);
        const ev = await ringEvents(session, kit.plan, 'SETTINGS_LOAD');
        expect(ev.length).toBe(1);
        expect(ev[0]!.args.slice(0, 3), 'state, copy, seq').toEqual(want.load);
        expect(ev[0]!.flags & 1, 'error event').toBe(want.error);
        const [n, v] = await mb.get('k');
        expect([n, v.toString()], 'get gives its value').toEqual(want.value);
        await mb.put('k2', text('new'));
        await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
        const after = await kit.after(`copies-${want.name}`, session.cardPath!);
        const [target, keep] = [s.setB, s.setA];
        const rec = decodeSettings(sectorOf(after, target));
        expect(await written(), `the next write goes to the copy at sector ${target}`).toEqual([target]);
        expect(rec?.seq).toBe(want.seq);
        expect(settingsMap(rec).k2).toBe(text('new').toString('hex'));
        expect(sectorOf(after, keep).equals(sectorOf(session.cardPath!, keep)), 'the other copy is untouched').toBe(
          true,
        );
      },
    );
  });
}

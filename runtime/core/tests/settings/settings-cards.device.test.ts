import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import type { DkSession } from '@digi/dk-session';
import { describe, expect, test } from 'vitest';
import { bootInfo, buildCard, E_NOSTORE, statusOf } from '@digi/regression/runtime/card-kit.ts';
import { rpc, SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { DEVICES } from '@digi/regression/runtime/devices.ts';
import { E_SET_OFF, sectorOf, settingsMailbox, settingsRecord } from '@digi/regression/runtime/project-kit.ts';
import {
  byte,
  DELAY_MS,
  MODULE,
  padded,
  PATTERN,
  ROUTE,
  SECTOR,
  settingsCase,
  storeHeader,
  suiteName,
} from './settings-support.ts';
import { BOOT_LEVEL } from '@digi/dk-session/gen_host_dir';
import { SYSTEM_PACKAGE_STORE_PATH } from '@digi/regression/runtime/system-package.ts';

const { SAFE_CORE_ONLY, CORE_UNREADABLE } = BOOT_LEVEL;

/**
 * The settings store, part 3 (see settings.device.test.ts): a store that overlaps the settings area, and a card with the older header B.
 * The runtime's answers to put, get and remove, the load event and the CAPACITY figures of a store that overlaps are host tests (`settings_tests.rs`);
 * the dump decoding is `storedump.rs`' tests. Here the firmware shows the USB answers, the untouched card and the repaired header B.
 */

for (const device of DEVICES) {
  describe(suiteName(device), () => {
    const { s, first, headerB, limit, sectors, kit, written } = settingsCase(device);

    test('overlap: an older store with a file in the settings area turns the settings store off', async () => {
      // an older store (allocator ended at sectors - 3) with a file "x" from 2 sectors before header B for 4 sectors: it covers header B and the first sector of settings copy A
      const entry = Buffer.alloc(256);
      entry.write('x', 0);
      const firstRel = limit - 2;
      entry.writeUInt32BE(firstRel, 160);
      entry.writeUInt32BE(4 * SECTOR, 164);
      const table = padded(entry);
      const foreign: Record<number, Buffer> = {
        [first]: storeHeader(1, zlib.crc32(table), 1, 1 + 1 + 4, sectors),
        [first + 1]: table,
      };
      for (let k = 0; k < 4; k++) foreign[first + firstRel + k] = PATTERN;
      // the store cannot be opened, so the system package cannot be read from it: the boot is core-only (no runtime UI), the settings store is core code
      const rt = await kit.boot(await kit.card({ name: 'overlap', selection: [], foreign }));
      expect(await kit.ready(true), 'the boot is core-only: the package is unreadable from the refused store').toMatchObject({
        safe: 1,
        reason: SAFE_CORE_ONLY,
        coreCause: CORE_UNREADABLE, // the headers hold a store that fails its check (E_BAD_STORE): stage 1 cannot read the file
      });
      await rpc(rt.session, 'run.mode', { mode: 'paused' }); // the protocol calls below step the emulator themselves
      const { session } = rt;
      const usb = <T>(run: (dk: DkSession) => Promise<T>) => kit.usb(rt, run);
      const cap = (await usb((dk) => dk.capacity())).SETTINGS!;
      expect([cap.limit, cap.used], 'CAPACITY SETTINGS has limit 0 and nothing used').toEqual([0, 0]);
      expect(await statusOf(() => usb(settingsRecord)), 'the SETTINGS command answers E_SET_OFF').toBe(E_SET_OFF);
      expect(
        await statusOf(() => usb((dk) => dk.list())),
        'the store is not used either (LIST answers E_NOSTORE)',
      ).toBe(E_NOSTORE);
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      const after = await kit.after('overlap', session.cardPath!);
      expect(await written(), 'nothing is written in the store range').toEqual([]);
      for (let k = 0; k < 4; k++)
        expect(
          sectorOf(after, first + firstRel + k).equals(PATTERN),
          `file sector ${k} is byte for byte as it was`,
        ).toBe(true);
    });

    test('oldcard: a card with the older header B at sectors - 3 boots, keeps its selection and repairs the header copy on an upload', async () => {
      const mod = kit.mod(MODULE); // the module in the store's kind layout (the path the card rig writes)
      const dir = path.join(kit.artifacts, 'newlayout');
      fs.mkdirSync(dir, { recursive: true });
      const newLayout = await buildCard(device, {
        dir,
        modules: [mod],
        selection: [mod.name],
      });
      const hdrA = sectorOf(newLayout, first);
      // the card as an older runtime left it: header A and a copy of it at sectors - 3 (the old header B), nothing at today's header B
      const rt = await kit.load('oldcard', {
        modules: [MODULE],
        selection: [mod.name],
        foreign: { [headerB]: Buffer.alloc(SECTOR), [first + sectors - 3]: hdrA },
      });
      const { session } = rt;
      const usb = <T>(run: (dk: DkSession) => Promise<T>) => kit.usb(rt, run);
      const info = await bootInfo(session, kit.plan);
      expect(
        [info.results, info.safe, info.count, info.seq],
        'the module of the selection loads, no safe mode, the selection is kept',
      ).toEqual([[0], 0, 1, 1]);
      const cap = await usb((dk) => dk.capacity());
      expect(cap.SETTINGS!.limit).toBe(8192);
      expect(
        (await usb((dk) => dk.list())).map((e) => e.path).sort(),
        'the store reports its file and the package',
      ).toEqual([mod.name, SYSTEM_PACKAGE_STORE_PATH].sort());
      await settingsMailbox(session, kit.plan).put(ROUTE, byte(1));
      await rt.ui.wait({ ms: DELAY_MS + 500, why: SETTLE });
      expect(
        (await written()).filter((x) => x !== s.selA && x !== s.selB),
        "a settings write goes to copy A only (besides the selection record's boot counter)",
      ).toEqual([s.setA]);
      // an upload repairs the lagging header copy: header B is written at today's place, over free space
      await usb((dk) =>
        dk.put(
          't.bin',
          Uint8Array.from({ length: 768 }, (_, i) => i & 255),
        ),
      );
      const after = await kit.after('oldcard', session.cardPath!);
      const hb = sectorOf(after, headerB);
      const ha = sectorOf(after, first);
      expect(hb.subarray(0, 4).toString(), 'header B at the new place').toBe('DKST');
      expect(hb.subarray(24, 28).equals(ha.subarray(24, 28)), 'a header of the same seq as A').toBe(true);
      expect((await usb((dk) => dk.list())).map((e) => e.path).sort(), 'the store lists both files and the package').toEqual(
        [mod.name, 't.bin', SYSTEM_PACKAGE_STORE_PATH].sort(),
      );
      kit.allowStoreWrites(await written());
    });
  });
}

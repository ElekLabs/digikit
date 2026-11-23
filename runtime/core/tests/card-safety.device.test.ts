import zlib from 'node:zlib';
import { CMD, LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect, test } from 'vitest';
import {
  blob,
  cardInfo,
  claimHeader,
  currentRecord,
  E_CARD_RO,
  E_INUSE,
  E_NOSTORE,
  E_RANGE,
  probeWrite,
  setSelection,
  statusOf,
} from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { NONE, opStatuses } from '@digi/regression/runtime/debug-kit.ts';
import { cardStore, deviceProfile } from '@digi/regression/runtime/device-data.ts';
import type { DkSession } from '@digi/dk-session';
import { logEvents, ofKind } from '@digi/regression/runtime/runtime-menu.ts';
import { rpc } from '@digi/regression/runtime/fx-live.ts';
import { BOOT_LEVEL } from '@digi/dk-session/gen_host_dir';

/**
 * Host coverage (the decisions): `fake_platform.rs` (range refusals, blank claim, foreign, too small, torn claim, cut claim), `store_tests.rs` (foreign refuses
 * store writes, ours without headers, blank header pair rule, cut sweeps). These cases keep what the booted image shows: the boot counter record, the
 * USB replies, the log events and the card writes seen from the emulated card.
 *
 * Card safety (was Python `run_card`, `run_card_small`; deleted: 6a5bb1ed:tests/digikit/scenarios/card-safety.toml, 6a5bb1ed:tests/digikit/scenarios/card-small.toml): the one card-write function refuses writes
 * outside the store range; a blank card works as before; foreign bytes in the range make the card read-only for the runtime; a card with our store keeps
 * working; an upload on a blank card claims the range first, so a cut upload leaves a card the next boot calls ours; a card smaller than the store area has
 * no store. The write watch and the card writes are checked as `common_checks_upload`.
 * Also covers card-torn (was Python `run_card_torn`, deleted: c608bbf5:tests/digikit/scenarios/card-torn.toml): a power cut inside one sector write of the first
 * writes to a blank card.
 */

const MOD = 'modules/a.dkm';
const STRAY = 100; // relative sector of the foreign bytes
const PAT = Buffer.from(Array.from({ length: 512 }, (_, i) => (i * 7 + 3) & 0xff)); // neither all 0x00 nor all 0xFF
const { SAFE_CORE_ONLY, CORE_MISSING } = BOOT_LEVEL;

/**
 * A boot of `card`, a card whose store holds no readable system package (a blank card, foreign bytes, a torn header): the boot is core-only, and its cause is `coreCause`
 * (CORE_MISSING for every card here: with no store header, or a card too small for the store, stage 1 finds no package file; a header that fails its checks would be CORE_UNREADABLE). It
 * counts nothing and writes nothing of its own, never reaches the runtime UI and keeps the stock firmware; the card check, the card-write function
 * and the store commands are core code and answer as before. Returns the boot info; the run is paused for the protocol calls.
 */
async function coreOnlyBoot(h: CardRig, card: string, coreCause: number = CORE_MISSING) {
  const rt = await h.boot(card);
  const info = await h.ready(true);
  expect(info, 'the boot is core-only: the card holds no package').toMatchObject({ safe: 1, reason: SAFE_CORE_ONLY, coreCause });
  await rpc(rt.session, 'run.mode', { mode: 'paused' });
  return { rt, info, usb: <T>(run: (dk: DkSession) => Promise<T>) => h.usb(rt, run) };
}

cardRigs(
  'card safety',
  {
    devices: ['dt2'],
    dsp: 'zero',
    uploads: true,
    relaxFs: true, // the card carries the firmware's own writes: no filesystem check on the next boot
    roots: [...RT, 'core/ui'],
    symbols: CARD_SYMBOLS,
  },
  (h) => {
    const { first, end } = cardStore(h.device);

    test(
      'a blank card works as before and the card-write function refuses every write outside the store range',
      async () => {
        // A blank card has no store and so no package: the boot is core-only, it counts nothing and writes nothing.
        const { rt, usb } = await coreOnlyBoot(h, await h.card({ systemPackage: false }));
        const { session } = rt;
        const check = async () => cardInfo(session, h.plan);
        expect(await check(), 'the check found the range blank; nothing refused, claimed or written').toMatchObject({
          stateName: 'blank',
          firstNonblank: NONE,
          refused: 0,
          claims: 0,
          writes: 0,
        });
        expect(await h.storeWrites(), 'a core-only boot writes nothing in the store range').toEqual([]);
        // the full list of sizes and places is `every_write_outside_the_store_is_refused_for_range_and_counted_whatever_its_size` (fake_platform.rs)
        const bad: [number, number][] = [
          [first - 1, 512],
          [end, 512],
        ];
        const results: number[] = [];
        for (const [sector, n] of bad) results.push(await probeWrite(session, h.plan, sector, n));
        expect(results, 'a write outside the range is E_RANGE').toEqual(bad.map(() => E_RANGE));
        expect(await check(), 'the refusals are counted and wrote nothing').toMatchObject({
          refused: bad.length,
          refusedRange: bad.length,
          stateName: 'blank',
          writes: 0,
        });

        expect(await setSelection(session, h.plan, [MOD])).toBe(0);
        const recs = await h.records();
        expect(recs[0], 'two record sectors are written, no store header').not.toBeNull();
        expect(recs[0]).toEqual(recs[1]);
        // the selection write makes the record: sequence 1, no boot counter, both copies (two writes)
        expect(currentRecord(recs), 'the selection is in the record').toMatchObject({ seq: 1, counter: 0, paths: [MOD], flags: 0 });
        expect(await check(), 'the area is ours, no claim').toMatchObject({ stateName: 'ours', claims: 0, writes: 2 });

        const events = await usb((dk) => logEvents({ ...rt, dk }, h.plan));
        const checks = ofKind(events, LOG_KIND.CARD_CHECK);
        const refused = ofKind(events, LOG_KIND.CARD_REFUSED);
        expect(checks, 'the card check (blank) is no error event').toHaveLength(1);
        expect(checks[0]!.args[0]).toBe(2);
        expect(checks[0]!.flags & 1).toBe(0);
        expect(refused, 'one event per refused write').toHaveLength(bad.length);
        for (const e of refused) {
          expect(e.flags & 1, 'an error event').toBe(1);
          expect(e.args[2], 'reason: outside the range').toBe(1);
        }
      },
    );

    test(
      'a selection record and foreign bytes in both headers: the card is ours, the store is no store',
      async () => {
        const headerB = deviceProfile(h.device).card.store.header_b_sector;
        // (a valid record alone makes the area ours: fake_platform.rs and store_tests.rs; the upload on it: `an_ours_card_without_headers_is_an_empty_store_for_the_list_and_the_figures` (store_tests.rs))
        // foreign bytes in header A and B: the store is somebody's data
        const { rt, usb } = await coreOnlyBoot(h, await h.card({ selection: [], foreign: { [first]: PAT, [headerB]: PAT }, systemPackage: false }));
        const ci = await cardInfo(rt.session, h.plan);
        expect(ci.stateName, 'the card counts as ours').toBe('ours');
        await usb(async (dk) => {
          expect(await opStatuses(dk, ['begin', 'delete', 'list'])).toEqual([E_NOSTORE, E_NOSTORE, E_NOSTORE]);
          await dk.request(CMD.DATA, Buffer.alloc(14)); // DATA with no upload open: E_STATE, nothing staged
        });
        // The card holds a record (seq 1, counter 0). The store is no store, so no package can load: the boot is core-only and counts nothing. No write at all.
        expect(currentRecord(await h.records()), 'the record is as the card had it').toMatchObject({ seq: 1, counter: 0 });
        expect(await h.storeWrites(), 'nothing was written to the store range').toEqual([]);
      },
    );
  },
);

cardRigs(
  'card safety: foreign bytes',
  {
    devices: ['dt2'],
    uploads: true,
    roots: RT,
    symbols: CARD_SYMBOLS,
  },
  (h) => {
    test(
      'foreign bytes in the range make the card read-only: nothing written, no selection, no upload, the store commands answer E_NOSTORE',
      async () => {
        const { first } = cardStore(h.device);
        const { rt, usb } = await coreOnlyBoot(h, await h.card({ foreign: { [first + STRAY]: PAT }, systemPackage: false }));
        const { session } = rt;
        const hash = zlib.crc32(PAT);
        expect(
          await cardInfo(session, h.plan),
          'the check names the first non-blank sector and its crc32',
        ).toMatchObject({
          stateName: 'foreign',
          firstNonblank: STRAY,
          hash,
        });

        const rSel = await setSelection(session, h.plan, [MOD]);
        expect(rSel, 'the selection write is E_CARD_RO (probe and range answers: fake_platform.rs)').toBe(E_CARD_RO);
        expect(await cardInfo(session, h.plan)).toMatchObject({ refused: 1, writes: 0 });
        expect(await h.records(), 'no selection record was written').toEqual([null, null]);

        const info = await usb((dk) => dk.info());
        expect(info.card_read_only, 'INFO says the card is read-only').toBe(true);
        expect(info.free_bytes, 'INFO has no free bytes on a foreign area').toBe(0);
        expect(await usb((dk) => opStatuses(dk, ['list'])), 'LIST answers E_NOSTORE: a foreign area is no store').toEqual([E_NOSTORE]);
        expect(await usb((dk) => opStatuses(dk, ['begin', 'delete', 'clear']))).toEqual([E_INUSE, E_INUSE, E_INUSE]);

        const checks = ofKind(await usb((dk) => logEvents({ ...rt, dk }, h.plan)), LOG_KIND.CARD_CHECK);
        expect(checks, 'the log reports what the check found').toHaveLength(1);
        expect(checks[0]!.flags & 1, 'an error event').toBe(1);
        expect(checks[0]!.args.slice(0, 3)).toEqual([3, STRAY, hash]);
        expect(await h.storeWrites(), 'the runtime wrote nothing in the store range').toEqual([]);
      },
    );
  },
);

cardRigs('card safety: torn sectors', { dsp: 'zero', uploads: true, roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  const headerB = deviceProfile(h.device).card.store.header_b_sector;
  const claim = claimHeader(h.device);
  const torn = Buffer.concat([claim.subarray(0, 256), Buffer.alloc(256)]); // the cut came after half the sector

  test(
    "header B torn by the claim's first write, header A blank: the card reads blank, an upload answers E_NOSTORE and writes nothing; CLEAR recovers it",
    async () => {
      const { rt, usb } = await coreOnlyBoot(h, await h.card({ foreign: { [headerB]: torn }, systemPackage: false }));
      // The check read the area blank (the torn header B counts as blank); the boot is core-only and writes nothing, so the area stays blank. The torn
      // header is still no store: the upload answers E_NOSTORE and no header or data sector is written.
      expect((await cardInfo(rt.session, h.plan)).stateName).toBe('blank');
      expect(await statusOf(() => usb((dk) => dk.put('modules/b.dkm', blob(3_000, 31))))).toBe(E_NOSTORE);
      expect(await h.storeWrites(), 'nothing written in the store range').toEqual([]);
      expect(await statusOf(() => usb((dk) => dk.clear())), 'CLEAR').toBe(0);
      expect(await statusOf(() => usb((dk) => dk.put('modules/b.dkm', blob(3_000, 31)))), 'the next upload commits').toBe(0);
      expect((await usb((dk) => dk.list())).map((e) => e.path)).toContain('modules/b.dkm');
    },
  );
});

// A card smaller than the store area (the emulator's `small` identity reports 0x3B0000 sectors, below the area of both devices): no store.
cardRigs(
  'card-small',
  {
    initial: 'small',
    relaxFs: true,
    env: { DIGI_CARD_IDENTITY: 'small' },
    uploads: true,
    roots: [...RT, 'core/ui'],
    symbols: CARD_SYMBOLS,
  },
  (h) => {
    test(
      'a card smaller than the store area has no store: nothing is read or written there and uploads answer E_NOSTORE',
      async () => {
        const info = await h.ready(true);
        const { session, dk } = h.runtime;
        expect(
          await cardInfo(session, h.plan),
          'the check says too small; nothing refused, claimed or written',
        ).toMatchObject({
          stateName: 'too small',
          refused: 0,
          claims: 0,
          writes: 0,
        });
        expect(info, 'a card with no store holds no package: the boot is core-only').toMatchObject({ safe: 1, reason: SAFE_CORE_ONLY, coreCause: CORE_MISSING });

        const checks = ofKind(await logEvents(h.runtime, h.plan), LOG_KIND.CARD_CHECK);
        expect(checks, 'one error event for the check (state too small)').toHaveLength(1);
        expect(checks[0]!.flags & 1).toBe(1);
        expect(checks[0]!.args[0]).toBe(5);

        await dk.info();
        expect(
          await opStatuses(dk, ['begin', 'delete', 'mkdir', 'clear', 'list']),
          'every store command is E_NOSTORE',
        ).toEqual(Array(5).fill(E_NOSTORE));

        // the stock firmware formats this blank card (writes below the store area), so only the store range is checked
        expect(await h.storeWrites(), 'the runtime wrote nothing in the store range').toEqual([]);
      },
    );
  },
);

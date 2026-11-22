import zlib from 'node:zlib';
import { CMD, LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect, test } from 'vitest';
import {
  blob,
  cardInfo,
  claimHeader,
  bootInfo,
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
import { openAbout } from '@digi/regression/runtime/flows/menu.ts';
import { cardStore, deviceProfile } from '@digi/regression/runtime/device-data.ts';
import type { Session } from '@digi/test-support';
import { logEvents, ofKind } from '@digi/regression/runtime/runtime-menu.ts';

/**
 * Host coverage (the decisions): `fake_platform.rs` (range refusals, blank claim, foreign, too small, torn claim, cut claim), `store_tests.rs` (foreign refuses
 * store writes, ours without headers, blank header pair rule, cut sweeps). These cases keep what the booted image shows: the boot counter record, the
 * USB replies, the log events, the ABOUT rows and the card writes seen from the emulated card.
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
const BOOT_OK = { safe: 0, count: 0, uiReached: 1 };

/**
 * What the boot counter has done by now, read while the run is paused: the stable flag, the record the next boot would use and the card writes.
 * The stable-period clear lands at an emulated time the test does not control, so the expected values are exact functions of the stable flag read
 * with them (`stable` is 0 or 1): the boot raises the counter (one write), the clear lowers it (one more write and one more sequence number).
 */
async function counterState(h: CardRig, session: Session) {
  const stable = (await bootInfo(session, h.plan)).stable;
  const rec = currentRecord(await h.records());
  const writes = (await cardInfo(session, h.plan)).writes;
  return { stable, rec, writes };
}

cardRigs(
  'card safety',
  {
    devices: ['dt2'],
    dsp: 'zero',
    uploads: true,
    relaxFs: true, // the card carries the firmware's own writes: no filesystem check on the next boot
    roots: RT,
    symbols: CARD_SYMBOLS,
  },
  (h) => {
    const { first, end } = cardStore(h.device);

    test(
      'a blank card works as before and the card-write function refuses every write outside the store range',
      async () => {
        const { session } = h.runtime;
        const check = async () => cardInfo(session, h.plan);
        // The boot counts itself from the first boot: on a blank card it writes selection copy A (the write marks the area ours, no header claim).
        const boot = await check();
        expect(boot, 'the check found the range blank; the boot then marked it ours with its record, nothing refused or claimed').toMatchObject({
          stateName: 'ours',
          firstNonblank: NONE,
          refused: 0,
          claims: 0,
        });
        const s0 = await counterState(h, session);
        expect(s0.rec, 'the boot created the record: seq 1, counter 1 (seq 2 and counter 0 once the stable period cleared it), no selection').toMatchObject({
          seq: 1 + s0.stable,
          counter: 1 - s0.stable,
          flags: 0,
          paths: [],
        });
        expect(s0.writes, 'one write for the boot, one more for the clear').toBe(1 + s0.stable);
        // the full list of sizes and places is `every_write_outside_the_store_is_refused_for_range_and_counted_whatever_its_size` (fake_platform.rs)
        const bad: [number, number][] = [
          [first - 1, 512],
          [end, 512],
        ];
        const results: number[] = [];
        for (const [sector, n] of bad) results.push(await probeWrite(session, h.plan, sector, n));
        expect(results, 'a write outside the range is E_RANGE').toEqual(bad.map(() => E_RANGE));
        expect(await check(), 'the refusals are counted').toMatchObject({
          refused: bad.length,
          refusedRange: bad.length,
          stateName: 'ours',
        });
        const s1 = await counterState(h, session);
        expect(
          [s1.rec?.seq, s1.rec?.counter, s1.writes],
          'the refusals wrote nothing: the record and the write count are the boot counter\'s alone',
        ).toEqual([1 + s1.stable, 1 - s1.stable, 1 + s1.stable]);

        expect(await setSelection(session, h.plan, [MOD])).toBe(0);
        const recs = await h.records();
        expect(recs[0], 'two record sectors are written, no store header').not.toBeNull();
        expect(recs[0]).toEqual(recs[1]);
        const s2 = await counterState(h, session);
        // the selection write takes the next sequence number and keeps the counter; it writes both copies (two writes)
        expect(s2.rec, 'the selection is in the record').toMatchObject({ paths: [MOD], flags: 0 });
        expect([s2.rec?.seq, s2.rec?.counter, s2.writes], 'seq, counter and writes after the selection write').toEqual([
          2 + s2.stable,
          1 - s2.stable,
          3 + s2.stable,
        ]);
        expect(await check(), 'the area is ours, no claim').toMatchObject({ stateName: 'ours', claims: 0 });

        const events = await logEvents(h.runtime, h.plan);
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
        const rt = await h.boot(await h.card({ selection: [], foreign: { [first]: PAT, [headerB]: PAT } }));
        await h.ready();
        const ci = await cardInfo(rt.session, h.plan);
        expect(ci.stateName, 'the card counts as ours').toBe('ours');
        expect(await opStatuses(rt.dk, ['begin', 'delete', 'list'])).toEqual([E_NOSTORE, E_NOSTORE, E_NOSTORE]);
        await rt.dk.request(CMD.DATA, Buffer.alloc(14)); // DATA with no upload open: E_STATE, nothing staged
        // The card holds a record (seq 1, counter 0): the boot raises it into copy B (seq 2, counter 1) and the stable-period clear writes copy A (seq 3,
        // counter 0). Those are the only writes: no header, no data.
        const { selA, selB } = cardStore(h.device);
        const st = await counterState(h, rt.session);
        expect([st.rec?.seq, st.rec?.counter, st.writes]).toEqual([2 + st.stable, 1 - st.stable, 1 + st.stable]);
        expect(await h.storeWrites(), 'nothing was written to the store range besides the selection record').toEqual(
          st.stable ? [selA, selB] : [selB],
        );
      },
    );
  },
);

cardRigs(
  'card safety: foreign bytes',
  {
    devices: ['dt2'],
    uploads: true,
    roots: [...RT, 'core/digikit-menu'],
    symbols: [...CARD_SYMBOLS, 'DK_ABOUT', 'DK_ABOUT_N'],
  },
  (h) => {
    test(
      'foreign bytes in the range make the card read-only: nothing written, no selection, no upload, ABOUT says so',
      async () => {
        const { first } = cardStore(h.device);
        const rt = await h.boot(await h.card({ foreign: { [first + STRAY]: PAT } }));
        await h.ready();
        const { session, dk } = rt;
        const hash = zlib.crc32(PAT);
        expect(
          await cardInfo(session, h.plan),
          'the check names the first non-blank sector and its crc32',
        ).toMatchObject({
          stateName: 'foreign',
          firstNonblank: STRAY,
          hash,
        });
        expect(await bootInfo(session, h.plan), 'the boot runs normally').toMatchObject(BOOT_OK);

        const rSel = await setSelection(session, h.plan, [MOD]);
        expect(rSel, 'the selection write is E_CARD_RO (probe and range answers: fake_platform.rs)').toBe(E_CARD_RO);
        expect(await cardInfo(session, h.plan)).toMatchObject({ refused: 1, writes: 0 });
        expect(await h.records(), 'no selection record was written').toEqual([null, null]);

        const info = await dk.info();
        expect(info.card_read_only, 'INFO says the card is read-only').toBe(true);
        expect(info.free_bytes, 'INFO has no free bytes on a foreign area').toBe(0);
        expect(await opStatuses(dk, ['list']), 'LIST answers E_NOSTORE: a foreign area is no store').toEqual([E_NOSTORE]);
        expect(await opStatuses(dk, ['begin', 'delete', 'clear'])).toEqual([E_INUSE, E_INUSE, E_INUSE]);

        const checks = ofKind(await logEvents(rt, h.plan), LOG_KIND.CARD_CHECK);
        expect(checks, 'the log reports what the check found').toHaveLength(1);
        expect(checks[0]!.flags & 1, 'an error event').toBe(1);
        expect(checks[0]!.args.slice(0, 3)).toEqual([3, STRAY, hash]);

        const rows = await openAbout(rt, h.plan);
        expect(rows).toEqual(expect.arrayContaining(['CARD READ-ONLY', `NOT OURS AT ${STRAY}`, 'NO BOOT COUNTER']));
        for (const row of rows) expect(row.length, row).toBeLessThanOrEqual(21);
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
      const rt = await h.boot(await h.card({ foreign: { [headerB]: torn } }));
      await h.ready();
      // The check read the area blank (the torn header B counts as blank); the boot then wrote its record, so the area is ours. The torn header is
      // still no store: the upload answers E_NOSTORE and no header or data sector is written (only the selection record the boot counts in).
      expect((await cardInfo(rt.session, h.plan)).stateName).toBe('ours');
      expect(await statusOf(() => rt.dk.put('modules/b.dkm', blob(3_000, 31)))).toBe(E_NOSTORE);
      const { selA, selB } = cardStore(h.device);
      const st = await counterState(h, rt.session);
      expect(await h.storeWrites(), 'nothing written in the store range besides the selection record (A by the boot, B by the stable clear)').toEqual(
        st.stable ? [selA, selB] : [selA],
      );
      expect(await statusOf(() => rt.dk.clear()), 'CLEAR').toBe(0);
      expect(await statusOf(() => rt.dk.put('modules/b.dkm', blob(3_000, 31))), 'the next upload commits').toBe(0);
      expect((await rt.dk.list()).map((e) => e.path)).toContain('modules/b.dkm');
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
    roots: RT,
    symbols: CARD_SYMBOLS,
  },
  (h) => {
    test(
      'a card smaller than the store area has no store: nothing is read or written there and uploads answer E_NOSTORE',
      async () => {
        await h.ready();
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
        expect(await bootInfo(session, h.plan), 'the boot goes on: no safe mode, reason none (no store is not a read failure)').toMatchObject({
          ...BOOT_OK,
          reason: 0,
        });

        const checks = ofKind(await logEvents(h.runtime, h.plan), LOG_KIND.CARD_CHECK);
        expect(checks, 'one error event for the check (state too small)').toHaveLength(1);
        expect(checks[0]!.flags & 1).toBe(1);
        expect(checks[0]!.args[0]).toBe(5);

        await dk.info();
        expect(
          await opStatuses(dk, ['begin', 'delete', 'mkdir', 'clear', 'list']),
          'every store command is E_NOSTORE',
        ).toEqual(Array(5).fill(E_NOSTORE));
        const { STORE } = await dk.capacity();
        expect(STORE).toMatchObject({ limit: 0, used: 0 });

        // the stock firmware formats this blank card (writes below the store area), so only the store range is checked
        expect(await h.storeWrites(), 'the runtime wrote nothing in the store range').toEqual([]);
      },
    );
  },
);

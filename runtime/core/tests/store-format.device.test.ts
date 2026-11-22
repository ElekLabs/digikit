import { createHash } from 'node:crypto';
import fs from 'node:fs';
import zlib from 'node:zlib';
import { fileHashHex } from '@digi/dk-session';
import { BOOT_LEVEL } from '@digi/dk-session/gen_host_dir';
import { STATUS } from '@digi/dk-session/gen_proto';
import type { PatchedSession } from '@digi/regression/support.ts';
import { expect, test } from 'vitest';
import { blob, bootInfoLoaded, cardInfo, readSectors, statusOf } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { CARD_TIMEOUT } from '@digi/regression/runtime/debug-kit.ts';
import { cardStore, deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { liveCheck as check } from '@digi/regression/runtime/artifacts.ts';
import { rpc } from '@digi/regression/runtime/fx-live.ts';

/**
 * What a card with real bytes shows the booted image, both devices. The decisions are host tests: `store_tests.rs` (a flipped byte: E_SHA and VERIFY; a
 * version 3 store is empty and the first upload makes version 4), the loader's tests (E_SHA ends a module's load), `dktransfer.test.ts` (Transfer installs
 * services), `default-modules.test.ts` (`installDefaults`); `patchkit store dump` decoding is the unit tests of `storedump.rs`.
 *  - A flipped byte on the card: VERIFY says another hash, the module with that byte is not loaded.
 *  - The flash scenario: a card with a real version 3 store, a module and a selection. The new runtime boots, the first upload makes a version 4 store, a
 *    restart loads the module from the kept selection.
 * The END with a wrong hash is in upload.device.test.ts ("a wrong file hash: E_HASH").
 */

const SECTOR = 512;
const MOD_PATH = 'modules/c-demo.dkm';
const sectorsOf = (n: number) => Math.ceil(n / SECTOR);
const padded = (b: Buffer) => Buffer.concat([b, Buffer.alloc(sectorsOf(b.length) * SECTOR - b.length)]);

/** The bytes the previous runtime left for one module: header A and B (version 3), a table of one entry with the file's SHA-256, the file at sector 2. */
function version3Store(device: 'dt2' | 'dn2', file: Buffer): Record<number, Buffer> {
  const { first, end } = cardStore(device);
  const headerB = deviceProfile(device).card.store.header_b_sector;
  const entry = Buffer.alloc(256);
  entry.write(MOD_PATH, 0);
  entry.writeUInt32BE(2, 160);
  entry.writeUInt32BE(file.length, 164);
  createHash('sha256').update(file).digest().copy(entry, 168);
  const table = padded(entry);
  const head = Buffer.alloc(SECTOR);
  head.write('DKST', 0);
  [3, 1, 1, 1, zlib.crc32(table), 1, 2 + sectorsOf(file.length), end - first].forEach((v, i) =>
    head.writeUInt32BE(v >>> 0, 4 + 4 * i),
  );
  head.writeUInt32BE(zlib.crc32(head.subarray(0, 508)), 508);
  return { [first]: head, [headerB]: head, [first + 1]: table, [first + 2]: padded(file) };
}

const { SAFE_CORE_ONLY, CORE_MISSING } = BOOT_LEVEL;
const E_PROTO = STATUS.find(([, name]) => name === 'E_PROTO')![0];

cardRigs(
  'store format 4',
  { dsp: 'zero', uploads: true, relaxFs: true, roots: RT, symbols: CARD_SYMBOLS },
  (h) => {
    const store = cardStore(h.device);
    /** The modules the runtime reports loaded (protocol calls run on the paused daemon). */
    const names = async (rt: PatchedSession) => (await h.usb(rt, (dk) => dk.modules())).map((m) => m.name);
    const demo = h.mod('test/c-demo');
    const bytes = () => fs.readFileSync(demo.file);

    test(
      'a flipped byte on the card: VERIFY reports it and the module is not loaded',
      async () => {
        const good = bytes();
        let rt: PatchedSession = await h.load('hash-good', { modules: [demo], selection: [demo.name] });
        expect(await bootInfoLoaded(rt.session, h.plan), 'the builder\'s hash is the runtime\'s: the module loads').toMatchObject({
          count: 1,
          loaded: 1,
          safe: 0,
        });
        check('VERIFY of the module equals the host hash', (await h.usb(rt, (dk) => dk.verify(demo.name))) === fileHashHex(good));
        // a second file, as a sample is: uploaded by the host over the protocol, verified, listed
        const sample = blob(5000, 9);
        expect(await statusOf(() => h.usb(rt, (dk) => dk.put('samples/s', sample)))).toBe(0);
        check('VERIFY of an uploaded file equals the host hash', (await h.usb(rt, (dk) => dk.verify('samples/s'))) === fileHashHex(sample));
        expect((await h.usb(rt, (dk) => dk.list())).find((e) => e.path === 'samples/s')?.hash).toBe(fileHashHex(sample));
        const after = await h.after('good');
        const ls = (await h.ls('good')).entries;
        const abs = (p: string) => store.first + ls.find((e) => e.path === p)!.firstSector;

        // flip one byte of the module on the card (the load's E_SHA and VERIFY of the flipped bytes: `store_tests.rs`, the loader's tests)
        const flipped = (p: string, at: number) => {
          const s = Buffer.from(readSectors(after, abs(p), 1));
          s[at] = s[at]! ^ 0x10;
          return s;
        };
        rt = await h.load('hash-bad', {
          base: after,
          foreign: { [abs(demo.name)]: flipped(demo.name, 300) },
        });
        const badModule = Buffer.from(good);
        badModule.writeUInt8(badModule.readUInt8(300) ^ 0x10, 300);
        check('VERIFY of the module says what the card holds now', (await h.usb(rt, (dk) => dk.verify(demo.name))) === fileHashHex(badModule));
        check('and that is not the host hash', fileHashHex(badModule) !== fileHashHex(good));
        const info = await bootInfoLoaded(rt.session, h.plan);
        expect(info, 'the selected module is not loaded').toMatchObject({ count: 1, loaded: 0 });
        expect(info.results, 'its load fails the hash check (E_SHA)').toEqual([-4]);
        expect(await names(rt), 'no module is registered').toEqual([]);
      },
    );

    test(
      'flash scenario: a version 3 store counts as empty, so the boot is core-only; the first upload makes a version 4 store and a restart keeps it',
      async () => {
        const file = bytes();
        const v3 = version3Store(h.device as 'dt2' | 'dn2', file);

        // boot: a version 3 store holds no package, so the boot is core-only (no runtime UI); the old store is no store (the card's reading of it: `store_tests.rs`)
        const card = await h.card({ selection: [MOD_PATH], foreign: v3, systemPackage: false });
        let rt: PatchedSession = await h.boot(card);
        expect(await h.ready(true), 'the boot is core-only: the package is missing, nothing is loaded').toMatchObject({
          safe: 1,
          reason: SAFE_CORE_ONLY,
          coreCause: CORE_MISSING,
          loaded: 0,
        });
        await rpc(h.session, 'run.mode', { mode: 'paused' });
        expect((await cardInfo(rt.session, h.plan)).stateName, 'the card is ours').toBe('ours');
        expect(await h.usb(rt, (dk) => dk.list()), 'LIST is empty').toEqual([]);
        expect(await statusOf(() => h.usb(rt, (dk) => dk.modules())), 'MODULES is refused in the core-only boot').toBe(E_PROTO);

        // the first upload makes a version 4 store, which the plusdrive reader reads off the card
        expect(await statusOf(() => h.usb(rt, (dk) => dk.put(MOD_PATH, file))), 'the upload works').toBe(0);
        expect((await h.view('v4-first')).head, 'the card holds a version 4 store').toContain('version=4');

        // a restart: still core-only (no package); the version 4 store is read whole and the module is not loaded
        rt = await h.restart(rt, { coreOnly: true });
        expect(await h.ready(true), 'the next boot is core-only too').toMatchObject({ safe: 1, reason: SAFE_CORE_ONLY, coreCause: CORE_MISSING, loaded: 0 });
        await rpc(h.session, 'run.mode', { mode: 'paused' });
        expect((await h.view('v4-restart')).head, 'the version 4 store is kept').toContain('version=4');
      },
      CARD_TIMEOUT * 2,
    );
  },
);

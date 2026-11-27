import { BULK_CMD, BULK_STATUS } from '@digi/dk-session/gen_bulk';
import { BulkDeviceError, BulkSession, bulkRequest, parseBulkReply, type BulkLink } from '@digi/dk-session';
import { expect, test } from 'vitest';
import { pokeKnob } from '@digi/regression/runtime/guest-mailbox.ts';
import { rpc } from '@digi/regression/runtime/guest-access.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import {
  ufSuite,
  SAMPLES,
  E_EXISTS,
  pattern,
  hashU32,
  PAIR,
  MODES,
  speedMask,
} from './usb-fast-support.ts';

/**
 * digikit/usb-fast uploads through the TypeScript BulkSession over the emulator link (`EmuBulkLink`: the daemon's usb.bulk.in / usb.bulk.out): uploads to the DN2 store and
 * the DT2 +Drive with their hash checks, REPLACE and E_EXISTS, DEFER and COMMIT, the copy/write overlap, the speed switches, ABORT, a USB reset in the middle of a file,
 * frames that span receive buffers and the receive ring. The bulk pair is the default route; the first test also runs on the OUT-only interface (a separate link class
 * whose replies come over the SysEx poll). The parser, the nonce of INFO, the speed mask, the staging buffer and the session expiry are host-tested with the module's C
 * file on a fake SDK: tests/uf-host.c. The emulator does not model the USB controller: what it cannot show (enumeration, the real dTD engine, real throughput) is for
 * the hardware run.
 */

ufSuite('upload', ({ dt2, rig, rp, call, baseCard, state, expectFile, unchanged, connect, boot }) => {
  test.each(MODES)(
    '$label: uploads reach the DN2 store or the DT2 +Drive with their hash; INFO carries the nonce and the replies-over-SysEx flag; VERIFY is reserved; REPLACE and E_EXISTS',
    async (mode) => {
      const { dk, bulk } = await boot(mode);
      const before = (await state(dk, 'uf0')).files;
      expect(before.size, 'the seed files are there').toBeGreaterThanOrEqual(2);
      const info = await bulk.session.info();
      expect([info.version, info.dataMax, info.align, info.nonce.length]).toEqual([1, 65536, 512, 8]);
      expect(info.flags & 1, 'INFO flags bit 0: replies over SysEx only in OUT-only mode').toBe(mode.outOnly ? 1 : 0);

      // a file of several 64 KiB frames, a tail that is not a multiple of 512, and a one-byte file
      const big = pattern(200_003);
      expect(
        await bulk.session.put(SAMPLES, rp('Kits/big.bin'), big, { expected: hashU32(big, dt2) }),
        'END returns the device hash',
      ).toBe(hashU32(big, dt2));
      const one = Buffer.from([0x5a]);
      expect(await bulk.session.put(SAMPLES, rp('one.bin'), one, { expected: hashU32(one, dt2) })).toBe(
        hashU32(one, dt2),
      );
      let s = await state(dk, 'uf1');
      await expectFile(dk, s, 'Kits/big.bin', big, 'big');
      await expectFile(dk, s, 'one.bin', one, 'one byte');
      unchanged(before, s.files, ['Kits/big.bin', 'one.bin']);

      await expect(bulk.session.verify(0, 100), 'VERIFY is reserved').rejects.toMatchObject({
        code: BULK_STATUS.E_PROTO,
      });

      // no REPLACE: the existing file is E_EXISTS and stays as it was
      const clash = await bulk.session.put(SAMPLES, rp('old.bin'), pattern(900)).catch((e) => e);
      expect(clash, 'E_EXISTS').toBeInstanceOf(BulkDeviceError);
      expect((clash as BulkDeviceError).code).toBe(E_EXISTS);
      s = await state(dk, 'uf2');
      expect(s.files.get('old.bin'), 'the old file is as it was').toEqual(before.get('old.bin'));
      // a wrong expected hash is E_HASH at END and leaves nothing
      const bad = await bulk.session
        .put(SAMPLES, rp('bad.bin'), pattern(3000), { expected: hashU32(pattern(3000, 1), dt2) })
        .catch((e) => e);
      expect((bad as BulkDeviceError).code, 'E_HASH').toBe(BULK_STATUS.E_HASH);
      expect((await state(dk, 'uf3')).files.has('bad.bin')).toBe(false);

      // REPLACE swaps the file: the new bytes verify
      const fresh = pattern(9000, 9);
      await bulk.session.put(SAMPLES, rp('old.bin'), fresh, { expected: hashU32(fresh, dt2), replace: true });
      s = await state(dk, 'uf4');
      await expectFile(dk, s, 'old.bin', fresh, 'replaced');
      unchanged(before, s.files, ['Kits/big.bin', 'one.bin', 'old.bin']);
      await rig.session.expectNoFault('usb-fast uploads');
    },
    1_800_000,
  );

  test.each([PAIR])(
    '$label: a batch of files sent with DEFER is verified at each END, named by one COMMIT (DN2; the DT2 +Drive names them at END), and a deferred path can be replaced',
    async (mode) => {
      const { dk, bulk } = await boot(mode);
      const before = (await state(dk, 'ub0')).files;
      const batch: [string, Buffer][] = [
        ['batch/a.bin', pattern(3000, 1)],
        ['batch/b.bin', pattern(700, 2)],
        ['batch/c.bin', pattern(70_001, 3)],
        ['batch/d.bin', pattern(512, 4)],
        ['batch/e.bin', pattern(12_345, 5)],
        ['batch/f.bin', Buffer.from([0x33])],
      ];
      const pending: (number | undefined)[] = [];
      for (const [name, data] of batch) {
        const r = await bulk.session.put(SAMPLES, rp(name), data, { expected: hashU32(data, dt2), defer: true });
        expect(r.hash, `${name}: END returns the device hash`).toBe(hashU32(data, dt2));
        pending.push(r.pending);
      }
      // a path that is already pending is replaced by the next file of that path (DN2: the pending entry; DT2: the +Drive replace)
      const first = pattern(2048, 6);
      const second = pattern(4096, 7);
      await bulk.session.put(SAMPLES, rp('batch/dup.bin'), first, { expected: hashU32(first, dt2), defer: true });
      const last = await bulk.session.put(SAMPLES, rp('batch/dup.bin'), second, {
        expected: hashU32(second, dt2),
        defer: true,
        replace: true,
      });
      pending.push(last.pending);
      if (dt2) {
        expect(pending, 'the +Drive names its files at END: nothing is ever pending').toEqual(pending.map(() => 0));
      } else {
        expect(pending.slice(0, batch.length), 'the DN2 holds each verified file pending').toEqual(
          batch.map((_, i) => i + 1),
        );
        expect(last.pending, 'the replaced path took the place of the pending file').toBe(batch.length + 1);
        const hidden = (await state(dk, 'ub1')).files;
        expect(
          [...hidden.keys()].filter((n) => n.startsWith('batch/')),
          'a pending file is not listed',
        ).toEqual([]);
      }
      expect(await bulk.session.commit(), 'COMMIT names them all').toBe(0);
      expect(await bulk.session.commit(), 'a second COMMIT has nothing to do').toBe(0);
      const s = await state(dk, 'ub2');
      for (const [name, data] of batch) await expectFile(dk, s, name, data, name);
      await expectFile(dk, s, 'batch/dup.bin', second, 'the replaced pending path');
      unchanged(before, s.files, [...batch.map(([n]) => n), 'batch/dup.bin']);

      await rig.session.expectNoFault('usb-fast batch');
    },
    1_800_000,
  );

  test.each([PAIR])(
    '$label: copy/write overlap: the same device hash and the same card contents for files below, at and above the 512 KiB half, with and without DEFER',
    async (mode) => {
      const { dk, bulk } = await boot(mode);
      // sizes: one half and less (one buffer), a tail in the second half, exact halves, a tail that is not a multiple of 512, several flushes
      const files: [string, Buffer][] = [
        ['small', pattern(300_001, 21)],
        ['half', pattern(524_288, 22)],
        ['half-plus', pattern(524_288 + 70_001, 23)],
        ['one-mib', pattern(1_048_576, 24)],
        ['odd', pattern(1_000_003, 25)],
        ['several', pattern(3 * 1_048_576 + 700, 26)],
      ];
      for (const [name, data] of files) {
        const want = hashU32(data, dt2);
        expect(
          await bulk.session.put(SAMPLES, rp(`Ov/${name}`), data, { expected: want }),
          `${name}: END returns the device hash`,
        ).toBe(want);
      }
      // a batch: deferred files across the half size are verified at their END and named by one COMMIT
      for (const [name, data] of files.slice(2, 4)) {
        const r = await bulk.session.put(SAMPLES, rp(`Ov/d-${name}`), data, {
          expected: hashU32(data, dt2),
          defer: true,
        });
        expect(r.hash, `deferred ${name}`).toBe(hashU32(data, dt2));
      }
      expect(await bulk.session.commit(), 'COMMIT').toBe(0);
      const s = await state(dk, 'ov0');
      for (const [name, data] of files) await expectFile(dk, s, `Ov/${name}`, data, name);
      for (const [name, data] of files.slice(2, 4)) await expectFile(dk, s, `Ov/d-${name}`, data, `deferred ${name}`);
      await rig.session.expectNoFault('usb-fast overlap');
    },
    3_600_000,
  );

  test('the speed switches (hash once, overlapped read-back hash), both on and both off: the device hash is the host hash, and a corrupted read-back or a wrong hash refuses the commit', async () => {
    const data = pattern(300_011, 17); // more than four 64 KiB pieces (DN2) and nine 32 KiB pages (DT2)
    const more = pattern(70_001, 19);
    // the two middle combinations are the mask logic of the module (tests/uf-host.c: the speed switches make the mask)
    const cases: [string, number, number][] = [
      ['both on (the default)', 1, 1],
      ['both off', 0, 0],
    ];
    for (const [k, [label, a, c]] of cases.entries()) {
      const dk = await rig.pausedBoot(await baseCard());
      const bulk = await connect(dk);
      expect(await speedMask(dk), `${label}: all switches are on after a boot`).toBe(15);
      await speedMask(dk, 0, a);
      expect(await speedMask(dk, 2, c), `${label}: the mask`).toBe((a ? 1 : 0) | 2 | (c ? 4 : 0) | 8);
      // a corrupted read-back (DN2: the card holds another byte in its first write) or a wrong hash (DT2) refuses the commit
      if (dt2) {
        const bad = await bulk.session
          .put(SAMPLES, rp('Sw/bad.bin'), data, { expected: hashU32(pattern(300_011, 18), dt2) })
          .catch((e) => e);
        expect((bad as BulkDeviceError).code, `${label}: a wrong hash`).toBe(BULK_STATUS.E_HASH);
      } else {
        // the first card write: with core/sdhc-large on, the 300 KB file is one write (the third write would be the table commit)
        await pokeKnob(rig.session, rig.plan, 'DKRT_UPLOAD_FLIP', 1);
        const bad = await bulk.session
          .put(SAMPLES, rp('Sw/bad.bin'), data, { expected: hashU32(data, dt2) })
          .catch((e) => e);
        expect(bad, `${label}: a flipped byte`).toBeInstanceOf(BulkDeviceError);
        expect((bad as BulkDeviceError).code, `${label}: the hash error`).toBe(BULK_STATUS.E_HASH);
      }
      // no host hash (the received bytes are hashed whatever the switch says), then with it
      expect(await bulk.session.put(SAMPLES, rp('Sw/plain.bin'), data), `${label}: END returns the device hash`).toBe(
        hashU32(data, dt2),
      );
      expect(
        await bulk.session.put(SAMPLES, rp('Sw/hashed.bin'), more, { expected: hashU32(more, dt2) }),
        `${label}: END with a host hash`,
      ).toBe(hashU32(more, dt2));
      const s = await state(dk, `sw${k}`);
      expect(s.files.has('Sw/bad.bin'), `${label}: nothing of the refused file`).toBe(false);
      await expectFile(dk, s, 'Sw/plain.bin', data, `${label}: plain`);
      await expectFile(dk, s, 'Sw/hashed.bin', more, `${label}: hashed`);
      await rig.session.expectNoFault(`usb-fast switches ${label}`);
    }
  }, 3_600_000);

  // DN2 only (the knob counts the store writer's card writes). Coverage of the other card faults: upload-faults.device.test.ts (core/runtime-tests) flips each of the
  // three data writes of a small upload (E_HASH, nothing committed) and cuts or tears every write of the commit; this test adds the usb-fast geometry (two data writes, with the ring and large commands on and off) and the flipped table commit.
  test('DN2: a flipped byte in the second data write is E_HASH, and a flipped table-commit write is E_WRITE; nothing of the file is committed either way', async () => {
    if (dt2) return;
    const data = pattern(300_011, 23);
    // the 300 KB file is two card writes (the 256 KiB staging flush and the tail), the third write is the table commit
    const cases: [string, number, number, number][] = [
      ['ring and large commands off: the second data write', 0, 2, BULK_STATUS.E_HASH],
      ['ring and large commands on: the second data write', 1, 2, BULK_STATUS.E_HASH],
      ['ring and large commands off: the table commit', 0, 3, BULK_STATUS.E_WRITE],
      ['ring and large commands on: the table commit', 1, 3, BULK_STATUS.E_WRITE],
    ];

    for (const [k, [label, large, write, code]] of cases.entries()) {
      const dk = await rig.pausedBoot(await baseCard());
      const bulk = await connect(dk);
      await speedMask(dk, 1, large);
      expect(await speedMask(dk, 3, large), `${label}: the mask`).toBe(large ? 15 : 5);
      await pokeKnob(rig.session, rig.plan, 'DKRT_UPLOAD_FLIP', write);
      const bad = await bulk.session
        .put(SAMPLES, rp('Fl/bad.bin'), data, { expected: hashU32(data, dt2) })
        .catch((e) => e);
      expect(bad, `${label}: refused`).toBeInstanceOf(BulkDeviceError);
      expect((bad as BulkDeviceError).code, label).toBe(code);
      await pokeKnob(rig.session, rig.plan, 'DKRT_UPLOAD_FLIP', 0);
      expect(
        await bulk.session.put(SAMPLES, rp('Fl/ok.bin'), data, { expected: hashU32(data, dt2) }),
        `${label}: the next upload works`,
      ).toBe(hashU32(data, dt2));
      const s = await state(dk, `fl${k}`);
      expect(s.files.has('Fl/bad.bin'), `${label}: nothing of the refused file`).toBe(false);
      await expectFile(dk, s, 'Fl/ok.bin', data, `${label}: ok.bin`);
      await rig.session.expectNoFault(`usb-fast flip ${label}`);
    }
  }, 3_600_000);

  test.each([PAIR])(
    '$label: ABORT drops the open file and a cancel mid-transfer leaves the old file; a USB reset in the middle of a file leaves nothing and the next upload works',
    async (mode) => {
      const { dk, bulk } = await boot(mode);
      const before = (await state(dk, 'ua0')).files;
      await bulk.session.abort(); // nothing open: ok

      // a cancel after the first acknowledged frames: ABORT is sent, the destination is not created; a replace keeps the old file
      const half = pattern(400_000, 2);
      for (const [name, replace] of [
        ['aborted.bin', false],
        ['old.bin', true],
      ] as const) {
        const stop = new AbortController();
        const e = await bulk.session
          .put(SAMPLES, rp(name), half, {
            replace,
            signal: stop.signal,
            onProgress: (sent) => sent >= 130_000 && stop.abort(),
          })
          .catch((x) => x);
        expect((e as { kind?: string }).kind, `${name}: cancelled`).toBe('cancelled');
      }
      let s = await state(dk, 'ua1');
      expect(s.files.has('aborted.bin'), 'nothing at the aborted path').toBe(false);
      unchanged(before, s.files, []);
      const ok = pattern(20_000, 4);
      await bulk.session.put(SAMPLES, rp('aborted.bin'), ok, { expected: hashU32(ok, dt2) });
      s = await state(dk, 'ua2');
      await expectFile(dk, s, 'aborted.bin', ok, 'after the aborts');

      // a bus reset while the file is open (the host's request is lost): the module aborts the file and clears the parser; the host fails the upload and resyncs
      const reset = await connect(dk, { timeoutMs: 1500, mode });
      const big = pattern(600_000, 6);
      const connectAddr = await mode.callback(call);
      let cut = false;
      const failed = await reset.session
        .put(SAMPLES, rp('reset.bin'), big, {
          replace: false,
          onProgress: (sent) => {
            if (!cut && sent >= 200_000) {
              cut = true;
              void rpc(rig.session, 'usb.bulk.in', { connect: connectAddr });
            }
          },
        })
        .catch((e) => e);
      expect(cut, 'the reset was delivered mid-file').toBe(true);
      expect(failed, 'the upload fails').toBeInstanceOf(Error);
      s = await state(dk, 'ua3');
      expect(s.files.has('reset.bin'), 'no partial file').toBe(false);
      expect([...s.files.keys()].sort(), 'the files are those of before and the one before the reset').toEqual(
        [...before.keys(), 'aborted.bin'].sort(),
      );
      // the host starts again on a fresh link: the same destination takes the file, so no session was left open
      const again = await connect(dk, { mode });
      await again.session.put(SAMPLES, rp('reset.bin'), ok, { expected: hashU32(ok, dt2) });
      s = await state(dk, 'ua4');
      await expectFile(dk, s, 'reset.bin', ok, 'after the reset');
      await rig.session.expectNoFault('usb-fast abort and reset');
    },
    1_800_000,
  );

  test.each([PAIR])(
    '$label: frames that span receive buffers and buffers with many frames, around 16 KiB and the 512-byte packet, with and without the zero-length packet',
    async (mode) => {
      const { dk } = await boot(mode);
      const before = (await state(dk, 'uz0')).files;
      // chunk sizes: 512 (528-byte frames, about 31 to a 16 KiB buffer), 16 KiB (a frame is 16 bytes over a buffer), 65536 (a frame spans five buffers). File sizes: the last
      // DATA frame is 12 + 4 + n bytes: n = 496 and 16368 make a frame of exactly 512 and 16384 bytes (the zero-length packet ends it; 16384 fills a buffer
      // and the packet gets a dTD of its own), 16369 and 16385 are one byte over.
      const sizes = [
        1,
        496,
        497,
        511,
        512,
        1000,
        16_368,
        16_369,
        16_384,
        16_385,
        16_368 + 512 * 3,
        65_536,
        65_536 + 496,
        100_000,
      ];
      let k = 0;
      for (const chunk of [512, 16_384, 65_536]) {
        const { session } = await connect(dk, { chunk, mode });
        for (const size of sizes) {
          const data = pattern(size, size + chunk);
          const got = await session.put(SAMPLES, rp(`f${chunk}-${size}.bin`), data, { expected: hashU32(data, dt2) });
          expect(got, `chunk ${chunk}, ${size} bytes: END hash`).toBe(hashU32(data, dt2));
          k++;
        }
      }
      const s = await state(dk, 'uz1');
      expect(s.files.size, 'every file is there').toBe(before.size + k);
      for (const [name, size, chunk] of [
        ['f512-496.bin', 496, 512],
        ['f16384-16368.bin', 16_368, 16_384],
        ['f65536-65536.bin', 65_536, 65_536],
        ['f512-100000.bin', 100_000, 512],
      ] as const)
        await expectFile(dk, s, name, pattern(size, size + chunk), name);

      // without the zero-length packet the device never sees a frame of exactly 512 bytes: no reply; the packet completes it
      const raw = (await connect(dk, { zlp: false, mode })).link;
      const rq = (cmd: number, seq: number, body: Buffer) => raw.send(bulkRequest(cmd, seq, body));
      const be32 = (n: number) => Buffer.from([n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
      const data = pattern(496, 77);
      await rq(
        BULK_CMD.BEGIN,
        1,
        Buffer.concat([be32(SAMPLES), Buffer.from(`${rp('nozlp.bin')}\0`), be32(data.length), Buffer.alloc(4)]),
      );
      expect(parseBulkReply((await raw.recv(20_000))!).status, 'BEGIN').toBe(0);
      await rq(BULK_CMD.DATA, 2, Buffer.concat([be32(0), data])); // the frame is 512 bytes: no packet follows
      for (let i = 0; i < 40; i++) await rig.runtime.ui.wait({ ms: 50, why: SETTLE });
      expect(await raw.recv(300), 'no reply while the transfer is open').toBeNull();
      await rpc(rig.session, 'usb.bulk.in', { zlp: true });
      const r = parseBulkReply((await raw.recv(20_000))!);
      expect([r.cmd, r.seq, r.status], 'the packet ends the transfer: DATA is answered').toEqual([BULK_CMD.DATA, 2, 0]);
      await rq(BULK_CMD.END, 3, Buffer.alloc(0));
      await rpc(rig.session, 'usb.bulk.in', { zlp: true }); // END is 12 bytes: this packet is an extra empty transfer the adapter drops
      const end = parseBulkReply((await raw.recv(20_000))!);
      expect([end.status, Buffer.from(end.body).readUInt32BE(0)], 'END').toEqual([0, hashU32(data, dt2)]);
      await rig.session.expectNoFault('usb-fast frames');
    },
    3_600_000,
  );

  test.each(MODES)(
    '$label: the receive ring: INFO reports the window and the host keeps that many DATA frames in flight, a bus reset with a full ring commits nothing, the switch off or a host that never asked INFO keeps 2',
    async (mode) => {
      const { dk, bulk } = await boot(mode);
      /** STATUS 'S' 1 [value]: the ring switch of core/usb-bulk, bit 1 of the mask (reads without a value). */
      const ring = async (value?: number) => {
        const r = await dk.request(0x37, Uint8Array.from(value === undefined ? [0x53] : [0x53, 1, value]));
        expect(r.status, 'STATUS S').toBe(0);
        return { on: (Buffer.from(r.body).readUInt32BE(0) >>> 1) & 1 };
      };
      /** The most DATA requests sent and not yet answered, seen at the link. */
      const meter = (link: BulkLink) => {
        const m = { now: 0, max: 0 };
        const send = link.send.bind(link);
        const recv = link.recv.bind(link);
        link.send = async (f: Uint8Array) => {
          await send(f);
          if (f[4] === BULK_CMD.DATA) m.max = Math.max(m.max, ++m.now);
        };
        link.recv = async (t: number, sig?: AbortSignal) => {
          const r = await recv(t, sig);
          if (r && r[4] === (BULK_CMD.DATA | 0x40)) m.now--;
          return r;
        };
        return m;
      };
      const before = (await state(dk, 'ur0')).files;
      expect(await ring(), 'the ring is on by default: 60 slots, 12 frames of 64 KiB').toEqual({ on: 1 });
      expect((await bulk.session.info()).window, 'INFO carries the window').toBe(12);
      const m1 = meter(bulk.link);
      const big = pattern(3 << 20, 21);
      expect(
        await bulk.session.put(SAMPLES, rp('ring.bin'), big, { expected: hashU32(big, dt2) }),
        'a file up with the window',
      ).toBe(hashU32(big, dt2));
      expect(m1.max, 'twelve DATA frames in flight').toBe(12);
      let s = await state(dk, 'ur1');
      await expectFile(dk, s, 'ring.bin', big, 'ring.bin');

      // a host that never asked INFO (an old host) keeps 2, whatever the device offers
      const old = new BulkSession(bulk.link);
      const m2 = meter(bulk.link);
      const small = pattern(1 << 20, 22);
      await old.put(SAMPLES, rp('old-host.bin'), small, { expected: hashU32(small, dt2) });
      expect(m2.max, 'no INFO read: two in flight').toBe(2);

      // a bus reset while the ring is full: nothing is committed, the next upload works on a new link
      const reset = await connect(dk, { timeoutMs: 1500, mode });
      const connectAddr = await mode.callback(call);
      let cut = false;
      const failed = await reset.session
        .put(SAMPLES, rp('ring-reset.bin'), pattern(4 << 20, 23), {
          onProgress: (sent) => {
            if (!cut && sent >= 1_500_000) {
              cut = true;
              void rpc(rig.session, 'usb.bulk.in', { connect: connectAddr });
            }
          },
        })
        .catch((e) => e);
      expect(cut, 'the reset was delivered mid-file').toBe(true);
      expect(failed, 'the upload fails').toBeInstanceOf(Error);
      s = await state(dk, 'ur2');
      expect(s.files.has('ring-reset.bin'), 'no partial file').toBe(false);
      const again = await connect(dk, { mode });
      const ok = pattern(300_000, 24);
      await again.session.put(SAMPLES, rp('ring-reset.bin'), ok, { expected: hashU32(ok, dt2) });
      s = await state(dk, 'ur3');
      await expectFile(dk, s, 'ring-reset.bin', ok, 'after the reset with a full ring');

      // the switch off: the old 8 slots, the host keeps 2; on again: 12
      expect(await ring(0), 'switch off').toEqual({ on: 0 });
      const off = await connect(dk, { mode });
      expect((await off.session.info()).window, 'INFO: no window').toBe(0);
      const m3 = meter(off.link);
      await off.session.put(SAMPLES, rp('ring-off.bin'), small, { expected: hashU32(small, dt2) });
      expect(m3.max, 'switch off: two in flight').toBe(2);
      expect(await ring(1), 'switch on').toEqual({ on: 1 });
      const on = await connect(dk, { mode });
      const m4 = meter(on.link);
      await on.session.put(SAMPLES, rp('ring-on.bin'), small, { expected: hashU32(small, dt2) });
      expect(m4.max, 'switch on: twelve in flight').toBe(12);
      s = await state(dk, 'ur4');
      for (const n of ['old-host.bin', 'ring-off.bin', 'ring-on.bin']) await expectFile(dk, s, n, small, n);
      unchanged(before, s.files, ['ring.bin', 'old-host.bin', 'ring-reset.bin', 'ring-off.bin', 'ring-on.bin']);
      await rig.session.expectNoFault('usb-fast receive ring');
    },
    3_600_000,
  );
});

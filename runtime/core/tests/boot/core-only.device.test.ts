import fs from 'node:fs';
import { fileHashHex } from '@digi/dk-session';
import { BOOT_LEVEL } from '@digi/dk-session/gen_host_dir';
import { expect, test } from 'vitest';
import { currentRecord } from '@digi/regression/runtime/card-record.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { rpc } from '@digi/regression/runtime/guest-access.ts';
import { coreFrame, introNow } from '@digi/regression/runtime/intro-frame.ts';
import { SYSTEM_PACKAGE_STORE_PATH, systemPackageFile } from '@digi/regression/runtime/system-package.ts';

/**
 * Core-only boots and their recovery on the firmware (the level ladder, the counter rules and RETRY are host tests of `runtime/core`: `level.rs`, `boot.rs`,
 * `package_counter_tests.rs`). What only the device shows: a boot that finds the counter at the threshold or no package reports core-only (`DkrtBootInfo.core_cause`)
 * and draws the core-only frame in place of the stock intro, the resident store and transport answer in that boot, core-only boots do not count (two in a row leave the
 * counter in the selection record where it was), an upload of `digikit/system.dksp` or a RETRY command clears the counter in that record, and the next boot runs the package.
 * The reason and cause codes and the threshold come from the generated ABI (`BOOT_LEVEL`).
 *
 * The counter is cleared by the same core path for an upload and for RETRY, and for a card that crashed as for one without the package: RETRY is run on the crash card
 * (the only one with a counter to clear); the missing-package card shows that nothing is counted and that the upload recovers it.
 */
const { SAFE_CORE_ONLY, CORE_CRASHES, CORE_MISSING, CORE_ONLY_AFTER } = BOOT_LEVEL;
const PACKAGE = SYSTEM_PACKAGE_STORE_PATH;

cardRigs('core-only boots and recovery', { uploads: true, roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  type Running = Parameters<typeof h.restart>[0];
  const package_ = () => fs.readFileSync(systemPackageFile());

  /** The counter of the selection record on the card: the card must hold one. */
  const counter = async (): Promise<number> => {
    const record = currentRecord(await h.records());
    expect(record, 'the card holds a selection record').not.toBeNull();
    return record!.counter;
  };

  /** The boot that just ran is core-only: its boot info and the core-only frame. */
  const expectCoreOnly = async (cause: number, frame: 'crashes' | 'missing') => {
    const info = await h.ready(true);
    expect(info, 'the boot is core-only, loads nothing and reports its cause').toMatchObject({
      safe: 1,
      reason: SAFE_CORE_ONLY,
      coreCause: cause,
      uiReached: 1,
      loaded: 0,
    });
    expect((await introNow(h.session)).equals(coreFrame(frame)), `the intro shows the core-only frame (${frame})`).toBe(true);
    // the protocol calls below step the emulator themselves
    await rpc(h.session, 'run.mode', { mode: 'paused' });
  };

  /** A core-only boot of `card`. */
  const coreOnly = async (card: string, cause: number, frame: 'crashes' | 'missing') => {
    const rt = await h.boot(card);
    await expectCoreOnly(cause, frame);
    return rt;
  };

  /** Two core-only boots in a row leave the counter at `expected`: the second boot reads the record the first one left. */
  const twiceCoreOnly = async (rt: Running, expected: number, cause: number, frame: 'crashes' | 'missing') => {
    expect(await counter(), 'the first core-only boot does not count itself').toBe(expected);
    const again = await h.restart(rt, { coreOnly: true });
    await expectCoreOnly(cause, frame);
    expect(await counter(), 'the second core-only boot does not count either').toBe(expected);
    return again;
  };

  /** The restart after a recovery: the package runs (a normal boot, no cause, the counter starts from zero) and the intro is not a reason frame. */
  const normalAfter = async (rt: Running) => {
    await h.restart(rt);
    const info = await h.ready();
    expect(info, 'the next boot runs the package: not safe, not core-only, counter 0').toMatchObject({
      safe: 0,
      reason: 0,
      coreCause: 0,
      counter: 0,
    });
    expect((await introNow(h.session)).equals(coreFrame('crashes')), 'no core-only frame').toBe(false);
  };

  test('at CORE_ONLY_AFTER the boot is core-only and stays so without counting; an upload of the package clears the counter', async () => {
    const first = await coreOnly(await h.card({ selection: [], counter: CORE_ONLY_AFTER }), CORE_CRASHES, 'crashes');
    const rt = await twiceCoreOnly(first, CORE_ONLY_AFTER, CORE_CRASHES, 'crashes');
    // the resident store and transport answer: the host lists the package that is on the card and uploads it again
    const listed = await h.usb(rt, (dk) => dk.list());
    expect(listed.map((e) => e.path), 'LIST works in a core-only boot and shows the package').toContain(PACKAGE);
    const bytes = package_();
    await h.usb(rt, (dk) => dk.put(PACKAGE, bytes));
    expect((await h.usb(rt, (dk) => dk.verify(PACKAGE))), 'the uploaded package is whole').toBe(fileHashHex(bytes));
    expect(await counter(), 'writing the package clears the counter').toBe(0);
    await normalAfter(rt);
  });

  test('RETRY clears the counter without an upload, and the next boot runs the package', async () => {
    const rt = await coreOnly(await h.card({ selection: [], counter: CORE_ONLY_AFTER }), CORE_CRASHES, 'crashes');
    const before = await h.usb(rt, (dk) => dk.list());
    await h.usb(rt, (dk) => dk.retry());
    expect(await counter(), 'RETRY clears the counter').toBe(0);
    expect(await h.usb(rt, (dk) => dk.list()), 'and writes no file').toEqual(before);
    await normalAfter(rt);
  });

  test('a card without the package is core-only at once, not counted; uploading the package recovers it', async () => {
    const first = await coreOnly(await h.card({ selection: [], systemPackage: false }), CORE_MISSING, 'missing');
    const rt = await twiceCoreOnly(first, 0, CORE_MISSING, 'missing');
    expect((await h.usb(rt, (dk) => dk.list())).map((e) => e.path), 'the store has no package').not.toContain(PACKAGE);
    const bytes = package_();
    await h.usb(rt, (dk) => dk.put(PACKAGE, bytes));
    expect((await h.usb(rt, (dk) => dk.list())).map((e) => e.path), 'the package is in the store now').toContain(PACKAGE);
    await normalAfter(rt);
  });
});

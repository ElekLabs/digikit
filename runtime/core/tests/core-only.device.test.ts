import fs from 'node:fs';
import { fileHashHex } from '@digi/dk-session';
import { expect, test } from 'vitest';
import { currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { rpc } from '@digi/regression/runtime/fx-live.ts';
import { coreFrame, introNow, safeFrame } from '@digi/regression/runtime/intro-frame.ts';
import { SYSTEM_PACKAGE_STORE_PATH, systemPackageFile } from '@digi/regression/runtime/system-package.ts';

/**
 * Core-only boots and their recovery on the firmware (the level ladder, the counter rules and RETRY are host tests of `runtime/core`: `level.rs`, `boot.rs`,
 * `package_counter_tests.rs`). What only the device shows: a boot that finds the counter at the threshold or no package reports core-only (`DkrtBootInfo.core_cause`)
 * and draws the core-only frame in place of the stock intro, the resident store and transport answer in that boot, an upload of `digikit/system.dksp` or a RETRY
 * command clears the counter in the selection record on the card, and the next boot runs the package.
 *
 * `CORE_ONLY_AFTER` (level.rs) is 4 and `SAFE_AFTER` 2. The first case boots at the counter below it and then at it, so a changed threshold fails here.
 */
const SAFE_COUNTER = 1;
const SAFE_CORE_ONLY = 7;
const CORE_CRASHES = 1;
const CORE_MISSING = 2;
const CORE_ONLY_AFTER = 4;
const PACKAGE = SYSTEM_PACKAGE_STORE_PATH;

cardRigs('core-only boots and recovery', { roots: RT, symbols: CARD_SYMBOLS }, (h) => {
  const package_ = () => fs.readFileSync(systemPackageFile());

  /** The core-only boot of `card`: its boot info and the core-only frame. */
  const coreOnly = async (card: string, cause: number, frame: 'crashes' | 'missing') => {
    const rt = await h.boot(card);
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
    return rt;
  };

  /** The restart after a recovery: the package runs (a normal boot, no cause, the counter starts from zero) and the intro is not a reason frame. */
  const normalAfter = async (rt: Awaited<ReturnType<typeof h.boot>>) => {
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

  test('the counter below CORE_ONLY_AFTER is safe mode; at it the boot is core-only, and an upload of the package clears the counter', async () => {
    const below = await h.card({ selection: [], counter: CORE_ONLY_AFTER - 1 });
    await h.boot(below);
    expect(await h.ready(true), 'one boot below the threshold: safe mode by the counter, not core-only').toMatchObject({
      safe: 1,
      reason: SAFE_COUNTER,
      coreCause: 0,
    });
    expect((await introNow(h.session)).equals(safeFrame('counter'))).toBe(true);

    const rt = await coreOnly(await h.card({ selection: [], counter: CORE_ONLY_AFTER }), CORE_CRASHES, 'crashes');
    const record = currentRecord(await h.records());
    expect(record?.counter, 'a core-only boot does not count itself').toBe(CORE_ONLY_AFTER);
    // the resident store and transport answer: the host lists the package that is on the card and uploads it again
    const listed = await h.usb(rt, (dk) => dk.list());
    expect(listed.map((e) => e.path), 'LIST works in a core-only boot and shows the package').toContain(PACKAGE);
    const bytes = package_();
    await h.usb(rt, (dk) => dk.put(PACKAGE, bytes));
    expect((await h.usb(rt, (dk) => dk.verify(PACKAGE))), 'the uploaded package is whole').toBe(fileHashHex(bytes));
    expect(currentRecord(await h.records())?.counter, 'writing the package clears the counter').toBe(0);
    await normalAfter(rt);
  });

  test('RETRY clears the counter without an upload, and the next boot runs the package', async () => {
    const rt = await coreOnly(await h.card({ selection: [], counter: CORE_ONLY_AFTER }), CORE_CRASHES, 'crashes');
    const before = await h.usb(rt, (dk) => dk.list());
    await h.usb(rt, (dk) => dk.retry());
    expect(currentRecord(await h.records())?.counter, 'RETRY clears the counter').toBe(0);
    expect(await h.usb(rt, (dk) => dk.list()), 'and writes no file').toEqual(before);
    await normalAfter(rt);
  });

  test('a card without the package is core-only at once, not counted; uploading the package recovers it', async () => {
    const rt = await coreOnly(await h.card({ selection: [], systemPackage: false }), CORE_MISSING, 'missing');
    expect(currentRecord(await h.records())?.counter ?? 0, 'a missing package is not a crash: nothing was counted').toBe(0);
    expect((await h.usb(rt, (dk) => dk.list())).map((e) => e.path), 'the store has no package').not.toContain(PACKAGE);
    const bytes = package_();
    await h.usb(rt, (dk) => dk.put(PACKAGE, bytes));
    await normalAfter(rt);
  });
});

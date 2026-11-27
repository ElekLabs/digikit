import { expect, test } from 'vitest';
import { HostDirectoryError } from '@digi/dk-session';
import { emulatorRead, readTopMenuItems, readViewStack, viewStackFacts, type Device } from '@digi/test-support';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import type { Rt } from '@digi/regression/runtime/panel-waits.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';
import { packageSymbol } from '@digi/regression/runtime/runtime-plan.ts';

/**
 * The DIGIKIT menu is part of the system package (docs/plans/resident-core-and-system-package.md, RC2s-3). What only the firmware shows: core/ui's hook at the end of the real
 * SETTINGS rebuild asks the runtime for the package's rows, and the package's menu appears and opens on the real views; with no package the same hook has nothing to add and the
 * firmware goes on. The menu's structure (rows from the data, the selection, the pages) is host tested with a fake runtime: `runtime/platform/ui/host/tests/menu_mechanism.rs` and
 * `menu.rs`; the hook, its null default and the table entries are `core/runtime:rust-tests` and `patchkit:rust-tests` (`syspkg.rs`, `menu_part`).
 * The first case reads the menu's own state (its ABOUT row count, `MENU_ABOUT_N` of the host directory) and the item count of the firmware's SETTINGS menu (the Menu object's item vector,
 * `readTopMenuItems`); the second boots with no package and reads the same count: the DIGIKIT row is the one item more of the first.
 */
const MAX_STACK = 32;

/** The number of items of the stock SETTINGS menu, with SETTINGS open and then closed again. */
async function settingsItems(rt: Rt): Promise<number> {
  const { session, ui } = rt;
  const facts = viewStackFacts(session.options.device as Device);
  if (!facts) throw new Error('the device has no [ui] facts');
  const read = emulatorRead(session);
  const depth = async () => (await readViewStack(read, facts, MAX_STACK)).length;
  const below = await depth();
  await ui.tap('SETTINGS', { until: { state: depth, done: (n: number) => n > below, what: 'SETTINGS on the view stack' } });
  const items = await readTopMenuItems(read, facts);
  await ui.tap('NO', { until: { drained: true } });
  return items;
}

/** The count of the normal boot, for the core-only case that follows it in this file. */
let normalItems = 0;

/** `DkrtBootInfo.reason` SAFE_CORE_ONLY and `core_cause` CORE_MISSING (abi.toml). */
const SAFE_CORE_ONLY = 7;
const CORE_MISSING = 2;

cardRigs('the DIGIKIT menu in the system package', { roots: [...RT, 'core/ui'], dsp: 'zero' }, (rig) => {
  test('a normal boot has the menu in the package block and SETTINGS > DIGIKIT opens it', async () => {
    await rig.first();
    const rt = await rig.load('package-menu', { selection: [] });
    const { session, ui } = rt;
    const lo = await packageSymbol(session, rig.plan, 'PACKAGE_LO');
    const hi = await packageSymbol(session, rig.plan, 'PACKAGE_HI');
    const about = await packageSymbol(session, rig.plan, 'MENU_ABOUT_N');
    expect(about >= lo && about < hi, 'the menu is a part of the package: its statics lie in the block the core placed it in').toBe(true);
    expect(await word(session, about), 'the menu has built no page yet').toBe(0);
    normalItems = await settingsItems(rt);
    expect(normalItems, 'SETTINGS has items').toBeGreaterThan(1);
    await openDigikitMenu(rt);
    await ui.tap('YES', { until: { state: () => word(session, about), done: (n) => n > 0, what: 'the package menu built its ABOUT page' } });
    await session.expectNoFault('SETTINGS > DIGIKIT opened from the package');
  });

  test('a core-only boot has no menu: the SETTINGS hook runs with no package and nothing of the package appears', async () => {
    const rt = await rig.boot(await rig.card({ selection: [], systemPackage: false }));
    expect(await rig.ready(true), 'no package on the card: the boot is core-only and loads nothing').toMatchObject({
      safe: 1,
      reason: SAFE_CORE_ONLY,
      coreCause: CORE_MISSING,
      loaded: 0,
    });
    const { session } = rt;
    await expect(packageSymbol(session, rig.plan, 'MENU_ABOUT_N'), 'no package, so no menu').rejects.toBeInstanceOf(HostDirectoryError);
    // the firmware rebuilds SETTINGS (the hook runs, finds no package and adds no row): the stock items only, one fewer than the normal boot's list that holds the DIGIKIT row
    expect(normalItems, 'the normal boot case ran first in this file').toBeGreaterThan(1);
    expect(await settingsItems(rt), 'no DIGIKIT row in the SETTINGS list of a core-only boot').toBe(normalItems - 1);
    await session.expectNoFault('SETTINGS rebuilt with no package');
    await expect(packageSymbol(session, rig.plan, 'MENU_ABOUT_N'), 'still no package').rejects.toBeInstanceOf(HostDirectoryError);
  });
});

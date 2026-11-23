import { expect, test } from 'vitest';
import { HostDirectoryError } from '@digi/dk-session';
import { cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { word } from '@digi/regression/runtime/fx-live.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';
import { packageSymbol } from '@digi/regression/runtime/runtime-plan.ts';

/**
 * The DIGIKIT menu is part of the system package (docs/plans/resident-core-and-system-package.md, RC2s-3). What only the firmware shows: core/ui's hook at the end of the real
 * SETTINGS rebuild asks the runtime for the package's rows, and the package's menu appears and opens on the real views; with no package the same hook has nothing to add and the
 * firmware goes on. The menu's structure (rows from the data, the selection, the pages) is host tested with a fake runtime: `runtime/platform/ui/host/tests/menu_mechanism.rs` and
 * `menu.rs`; the hook, its null default and the table entries are `core/runtime:rust-tests` and `patchkit:rust-tests` (`syspkg.rs`, `menu_part`).
 * The first case reads the menu's own state (its ABOUT row count, `MENU_ABOUT_N` of the host directory), not the screen.
 */
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
    const { session, ui } = rt;
    await expect(packageSymbol(session, rig.plan, 'MENU_ABOUT_N'), 'no package, so no menu').rejects.toBeInstanceOf(HostDirectoryError);
    // the same keys as above: the firmware rebuilds SETTINGS (the hook runs, finds no package and adds no row) and the cursor goes on without a fault
    await openDigikitMenu(rt);
    await ui.tap('NO', { until: { drained: true } });
    await session.expectNoFault('SETTINGS rebuilt with no package');
    await expect(packageSymbol(session, rig.plan, 'MENU_ABOUT_N'), 'still no package').rejects.toBeInstanceOf(HostDirectoryError);
  });
});

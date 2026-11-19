import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { REDRAW } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { word } from '@digi/regression/runtime/fx-live.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * What only the firmware shows of the menu refusal (docs/plans/extension-model.md E5b item 1): `dk_ui_menu_new` of the real core/ui reports the module that built the menu object
 * to the runtime (`dkrt_menu_built`), and the live job then refuses that module. Everything else is a unit test: the count by module record (`spans_tests.rs`
 * `a_menu_is_counted_for_the_module_that_holds_its_code_or_descriptor`, `the_job_reads_the_menu_count_of_its_own_record`), its reset at a load (`loader/records.rs`), the
 * order of the refusals (`reload.rs` `the_refusals_come_in_the_documented_order`) and the second read before the teardown (`a_menu_built_during_the_drain_gives_the_old_module_back`).
 *
 * The fixture `test/entry-demo` owns a menu: its `menu` function (the module's SETTINGS... page) builds a submenu object with `dk_ui_menu_new` and its own fill function. The DIGIKIT
 * menu's own page fills the module's rows only while it is open and counts for no module (E5c-2), so the module's own `dk_ui_menu_new` is the call that counts.
 * - before the page is opened the module has built no menu object: `RELOAD` goes through (a `menu` entry alone does not refuse);
 * - after SETTINGS... of the module in the DIGIKIT menu was opened and left, `UNLOAD` and `RELOAD` are refused with `E_RELOAD_MENUS`, the module is still in MODULES, and the counters did not move.
 *
 * Written, not run, and the key path is not looked at yet: `openModuleSettings` goes SETTINGS (the stock menu, whose last row the core/ui hook adds is DIGIKIT), DIGIKIT, MODULES, the module's row, RIGHT, the
 * panel row SETTINGS..., and `DIGIKIT_ROW_DOWNS`/`MODULE_ROW_DOWNS` are guesses: take one `dk:dev` screenshot of each step before the first run (docs/plans/extension-model.md, "Device proof").
 */
const MENU_ID = 'test/entry-demo';
const E_RELOAD_MENUS = loadCode('E_RELOAD_MENUS');
const DIGIKIT_ROW_DOWNS = 0;
const MODULE_ROW_DOWNS = 0;

/** Open the settings page of the module in the DIGIKIT menu and go back out to the main screen. */
async function openModuleSettings(rt: TestRuntime): Promise<void> {
  const tap = (key: string, why = REDRAW) => rt.ui.tap(key, { until: { ms: 300, why } });
  await tap('SETTINGS');
  for (let i = 0; i < DIGIKIT_ROW_DOWNS; i++) await tap('DOWN');
  await tap('YES'); // DIGIKIT
  for (let i = 0; i < 3; i++) await tap('DOWN'); // ABOUT, HEALTH, MEMORY, MODULES
  await tap('YES'); // MODULES
  for (let i = 0; i < MODULE_ROW_DOWNS; i++) await tap('DOWN');
  await tap('RIGHT'); // the side panel of the row
  await tap('DOWN'); // UNLOAD / LOAD, then SETTINGS...
  await tap('YES'); // SETTINGS...: the page is built and opened
  for (let i = 0; i < 6; i++) await tap('NO'); // back out of every menu
}

/** The menu objects the runtime counts for all module records (`DKRT_MENU_OBJS`, one byte per record, 8 records): only `test/entry-demo` builds any, so the sum is its count. */
async function menuObjects(rt: TestRuntime, symbol: number): Promise<number> {
  let n = 0;
  for (let i = 0; i < 2; i++) {
    const w = await word(rt.session, symbol + 4 * i);
    n += (w >>> 24) + ((w >>> 16) & 0xff) + ((w >>> 8) & 0xff) + (w & 0xff);
  }
  return n;
}

async function expectMenuRefusal(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-menus');
  const path = moduleStorePath(rt.session.options.device as 'dt2' | 'dn2', MENU_ID);
  await cliInstall(rt.session, MENU_ID, artifacts, 300_000, [DEBUG_MODULE]);
  const objs = planSymbol(await prepareRuntimePlan(rt.session, artifacts, ['DKRT_MENU_OBJS']), 'DKRT_MENU_OBJS');
  const before = (await rt.dk.modules()).map((m) => m.name).sort();
  expect(before, 'the menu module is loaded').toContain(MENU_ID);
  // no menu object yet (the count is the proof; a reload would also reset it): a reload goes through
  expect(await menuObjects(rt, objs), 'no menu object before the settings page is opened').toBe(0);
  expect(await rt.dk.reloadAndWait(path), 'before the settings page is opened').toEqual({ result: 0, unloaded: true });
  await openModuleSettings(rt);
  expect(
    await menuObjects(rt, objs),
    'the module built a menu object when its settings page opened: if this is 0 the key path of openModuleSettings is misaligned (take a dk:dev screenshot of each step)',
  ).toBeGreaterThan(0);
  const counters = (await rt.dk.reloadState()).step;
  expect(await rt.dk.unloadAndWait(path), 'after the settings page was built: UNLOAD').toEqual({ result: E_RELOAD_MENUS, unloaded: false });
  expect(await rt.dk.reloadAndWait(path), 'after the settings page was built: RELOAD').toEqual({ result: E_RELOAD_MENUS, unloaded: false });
  expect((await rt.dk.modules()).map((m) => m.name).sort(), 'the module is still loaded').toEqual(before);
  record(artifacts, 'menu-refusal', { before, step: counters });
}

runtimeTests('UNLOAD and RELOAD of a module that built a menu object are refused with E_RELOAD_MENUS', expectMenuRefusal, { dsp: 'zero' });

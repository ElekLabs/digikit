import { expect } from 'vitest';
import type { Device } from '@digi/test-support';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { word } from '@digi/regression/runtime/fx-live.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';

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
 * The hold of an open settings page of a module that only adds rows (`dkrt_menu_release`): `digikit/debug`'s page only adds rows (`dk_ui_menu_add_toggle`, `dk_ui_menu_add_text`). While its
 * page is open the DIGIKIT menu holds the module (`dkrt_menu_built`), so a reload is refused; after the page is left the hold is given back and the reload goes through. The logic is unit
 * tested (`menu.rs` `an_open_settings_page_holds_its_module_and_leaving_it_gives_the_hold_back`, `spans_tests.rs` `an_open_page_holds_the_module_and_the_release_lets_it_go`); the device step
 * shows the real DIGIKIT menu taking and giving back the hold.
 *
 * Key path (dk:dev screenshots): `openDigikitMenu` (SETTINGS, eight rows down is DIGIKIT), three rows down is MODULES; MODULES lists DEBUG then ENTRY-DEMO; RIGHT opens the row's side
 * panel (UNLOAD or LOAD, SETTINGS..., INFO); one row down is SETTINGS...
 */
const MENU_ID = 'test/entry-demo';
const E_RELOAD_MENUS = loadCode('E_RELOAD_MENUS');
/** The rows of MODULES with `digikit/debug` and `test/entry-demo` selected. */
const DEBUG_ROW = 0;
const MENU_ROW = 1;

const tapOf = (rt: TestRuntime) => (key: string) => rt.ui.tap(key, { until: { drained: true } });

/** What a key's last effect leaves in the firmware: the menu object count the next step reads (`drained` only means the key reached the UART, not that the UI task handled it). */
interface MenuCount {
  objs: number;
  done: (n: number) => boolean;
  what: string;
}
const tapUntilCount = (rt: TestRuntime, key: string, c: MenuCount) =>
  rt.ui.tap(key, { until: { state: () => menuObjects(rt, c.objs), done: c.done, what: c.what, withinMs: 10_000 } });

/** Open the settings page of the module on MODULES row `row` in the DIGIKIT menu; the page stays open. */
async function openModuleSettings(rt: TestRuntime, row: number, built: MenuCount): Promise<void> {
  const tap = tapOf(rt);
  await openDigikitMenu(rt);
  for (let i = 0; i < 3; i++) await tap('DOWN'); // ABOUT, HEALTH, MEMORY, MODULES
  await tap('YES'); // MODULES
  for (let i = 0; i < row; i++) await tap('DOWN');
  await tap('RIGHT'); // the side panel of the row
  await tap('DOWN'); // UNLOAD / LOAD, then SETTINGS...
  await tapUntilCount(rt, 'YES', built); // SETTINGS...: the page is built and opened
}

/** Back out of every menu to the main screen. */
async function leaveMenus(rt: TestRuntime, left: MenuCount): Promise<void> {
  const tap = tapOf(rt);
  for (let i = 0; i < 5; i++) await tap('NO');
  await tapUntilCount(rt, 'NO', left);
}

/** The menu holds the runtime counts for all module records (`DKRT_MENU_OBJS`, one byte per record, 8 records): `test/entry-demo`'s menu objects and an open page's hold. */
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
  const device = rt.session.options.device as Device;
  const path = moduleStorePath(device, MENU_ID);
  await cliInstall(rt.session, MENU_ID, artifacts, 300_000, [DEBUG_MODULE]);
  const objs = planSymbol(await prepareRuntimePlan(rt.session, artifacts, ['DKRT_MENU_OBJS']), 'DKRT_MENU_OBJS');
  const before = (await rt.dk.modules()).map((m) => m.name).sort();
  expect(before, 'the menu module is loaded').toContain(MENU_ID);
  // a rows-only settings page: digikit/debug's page holds its module while open, and gives the hold back when left
  const debugPath = moduleStorePath(device, DEBUG_MODULE);
  expect(await menuObjects(rt, objs), 'no hold before any page is opened').toBe(0);
  await openModuleSettings(rt, DEBUG_ROW, { objs, done: (n) => n === 1, what: 'the DEBUG page holds its module' });
  expect(await menuObjects(rt, objs), 'the open DEBUG settings page holds its module: if this is 0 the key path is misaligned').toBe(1);
  expect(await rt.dk.reloadAndWait(debugPath), 'RELOAD of debug while its settings page is open').toEqual({ result: E_RELOAD_MENUS, unloaded: false });
  await leaveMenus(rt, { objs, done: (n) => n === 0, what: 'leaving the DEBUG page gave the hold back' });
  expect(await menuObjects(rt, objs), 'leaving the page gave the hold back').toBe(0);
  expect(await rt.dk.reloadAndWait(debugPath), 'RELOAD of debug after its settings page was left').toEqual({ result: 0, unloaded: true });
  // no menu object yet (the count is the proof; a reload would also reset it): a reload goes through
  expect(await menuObjects(rt, objs), 'no menu object before the settings page is opened').toBe(0);
  expect(await rt.dk.reloadAndWait(path), 'before the settings page is opened').toEqual({ result: 0, unloaded: true });
  await openModuleSettings(rt, MENU_ROW, { objs, done: (n) => n > 0, what: 'the entry-demo page built a menu object' });
  await leaveMenus(rt, { objs, done: (n) => n > 0, what: 'the menu object stays after leaving' });
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

runtimeTests('UNLOAD and RELOAD are refused with E_RELOAD_MENUS while a module holds a menu object or an open settings page', expectMenuRefusal, { dsp: 'zero' });

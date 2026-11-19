import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { REDRAW } from '@digi/regression/runtime/fx-live.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * The menu refusal of a live job (docs/plans/extension-model.md E5b item 1): a menu keeps pointers into the code of the module that built it and menus are never freed, so a module with a menu object is
 * not unloaded or reloaded. `digikit/remote` has a settings page (the `menu` function of its header) that the DIGIKIT menu builds when the user opens it.
 * - before the page is opened the module has no menu object: `RELOAD` goes through (a header entry alone no longer refuses);
 * - after SETTINGS... of the module in the DIGIKIT menu was opened and left, `UNLOAD` and `RELOAD` are refused with `E_RELOAD_MENUS`, the module is still in MODULES, and the counters did not move.
 *
 * Written, not run, and the key path is not looked at yet: `openModuleSettings` goes SETTINGS (the stock menu, whose last row the core/ui hook adds is DIGIKIT), DIGIKIT, MODULES, the module's row, RIGHT, the
 * panel row SETTINGS..., and `DIGIKIT_ROW_DOWNS`/`MODULE_ROW_DOWNS` are guesses: take one `dk:dev` screenshot of each step before the first run (docs/plans/extension-model.md, "Device proof").
 */
const REMOTE_ID = 'digikit/remote';
const REMOTE_PATH = 'modules/remote.dkm';
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

async function expectMenuRefusal(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-menus');
  await cliInstall(rt.session, REMOTE_ID, artifacts, 300_000, [DEBUG_MODULE]);
  const before = (await rt.dk.modules()).map((m) => m.name).sort();
  expect(before, 'the remote module is loaded').toContain(REMOTE_ID);
  // no menu object yet: a reload goes through
  expect(await rt.dk.reloadAndWait(REMOTE_PATH), 'before the settings page is opened').toEqual({ result: 0, unloaded: true });
  await openModuleSettings(rt);
  const counters = (await rt.dk.reloadState()).step;
  expect(await rt.dk.unloadAndWait(REMOTE_PATH), 'after the settings page was built: UNLOAD').toEqual({ result: E_RELOAD_MENUS, unloaded: false });
  expect(await rt.dk.reloadAndWait(REMOTE_PATH), 'after the settings page was built: RELOAD').toEqual({ result: E_RELOAD_MENUS, unloaded: false });
  expect((await rt.dk.modules()).map((m) => m.name).sort(), 'the module is still loaded').toEqual(before);
  record(artifacts, 'menu-refusal', { before, step: counters });
}

runtimeTests('UNLOAD and RELOAD of a module whose settings page was built are refused with E_RELOAD_MENUS', expectMenuRefusal, { dsp: 'zero' });

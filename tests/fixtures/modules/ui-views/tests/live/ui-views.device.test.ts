import { RANGES } from '@digi/dk-session/gen_proto';
import { emulatorRead, readTopMenuItems, readViewStack, viewStackFacts, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { REDRAW } from '@digi/regression/runtime/panel-waits.ts';
import { installWithDebug, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';

/**
 * What only the firmware shows of the module screens of core/ui (docs/plans/track-manager.md, "core/ui"). The fixture `test/ui-views` is a content module: in `init` it adds two
 * SETTINGS rows (`dk_ui_settings_row`); the first opens a full-screen view (frame and text; YES opens the stock text input, NO closes), the second an overlay.
 * Everything else is a unit test: the pool, the row registry and the canvas mapping (`runtime/platform/ui/host/tests`), the observer list and the owner id of a context
 * (`core/runtime` `spans_tests.rs`, `cleanup_tests.rs`).
 *
 * The screen is read from firmware state, never from pixels: the view stack (Brain, view manager, list of nodes, the View of each node and its vptr: the `[ui]` facts of the device
 * files, `viewStackFacts`) and the item list of the stock SETTINGS menu (the Menu object's item vector; the offsets are the structs of `ports/fw/menu.toml`).
 * - the rows are in SETTINGS after the stock rows (two more items while the module is loaded), and YES on a row runs its function;
 * - the full-screen view is a new top view of the stack (its vptr is not the one below), draws on every frame and gets the keys; YES opens the stock NameView (another top view);
 *   YES on it calls `done` with the text, NO calls nothing; NO closes the view and `closed` runs;
 * - the overlay is a new top view and goes away on the module's request;
 * - an unload with the view open takes it off the stack (the stack is what it was before the view) and removes the rows (the item count is what it was before the module), without a fault.
 */
const REPORT = 0x0f;
/** The elements of a menu's item vector are `shared_ptr<MenuItem>`: two words (core/ui `rows_of`). */

const MAX_STACK = 32;

type Report = {
  row1: number;
  row2: number;
  fullOpen: number;
  overlayOpen: number;
  drawsFull: number;
  drawsOverlay: number;
  closedFull: number;
  closedOverlay: number;
  doneCalls: number;
  keyPresses: number;
  doneText: string;
};

const tapOf = (rt: TestRuntime) => (key: string, why = REDRAW) => rt.ui.tap(key, { until: { ms: 300, why } });

async function report(rt: TestRuntime): Promise<Report> {
  const reply = await rt.dk.request(REPORT);
  expect(reply.status, 'the module registered its command in init').toBe(0);
  const b = Buffer.from(reply.body);
  expect(b.length, 'report length').toBe(56);
  return {
    row1: b.readInt32BE(0),
    row2: b.readInt32BE(4),
    fullOpen: b.readUInt32BE(8),
    overlayOpen: b.readUInt32BE(12),
    drawsFull: b.readUInt32BE(16),
    drawsOverlay: b.readUInt32BE(20),
    closedFull: b.readUInt32BE(24),
    closedOverlay: b.readUInt32BE(28),
    doneCalls: b.readUInt32BE(32),
    keyPresses: b.readUInt32BE(36),
    doneText: b.subarray(40, 56).toString('latin1').replace(/\0+$/, ''),
  };
}

const until = (rt: TestRuntime, done: (r: Report) => boolean, message: string) =>
  pollEmulated(rt.session, () => report(rt), done, { emulatedMs: 20_000, message });

/** The firmware's view stack and SETTINGS menu, read from guest memory. */
function firmware(rt: TestRuntime) {
  const device = rt.session.options.device as Device;
  const facts = viewStackFacts(device);
  if (!facts) throw new Error(`${device} has no [ui] facts`);
  const read = emulatorRead(rt.session);
  /** The vtable pointers of the stack, the top one first. */
  const vptrs = async () => (await readViewStack(read, facts, MAX_STACK)).map((v) => v.vptr);
  /** How many items the top view's menu has (the top view must be a menu: SETTINGS). */
  const topMenuItems = () => readTopMenuItems(read, facts);
  return { vptrs, topMenuItems };
}

/** Open SETTINGS and press YES on row `row` (counted from the top) of a menu with `rows` rows. */
async function pressSettingsRow(rt: TestRuntime, row: number, rows: number): Promise<void> {
  const tap = tapOf(rt);
  await tap('SETTINGS');
  // the menu keeps its cursor from the last time and stops at its ends (it does not wrap): UP over every row puts the cursor on the first
  for (let i = 0; i < rows; i++) await tap('UP');
  for (let i = 0; i < row; i++) await tap('DOWN');
  await tap('YES');
}

/** Back out of every screen to the main screen. */
async function leave(rt: TestRuntime): Promise<void> {
  const tap = tapOf(rt);
  for (let i = 0; i < 4; i++) await tap('NO');
}

async function expectUiViews(rt: TestRuntime): Promise<void> {
  const { session, dk } = rt;
  const device = session.options.device as Device;
  const fw = firmware(rt);
  await expectRuntimeReady(session);
  const tap = tapOf(rt);
  // the events core/ui logs when a module view is pushed and when it leaves the stack, and for the naming screen: what the test waits on (the firmware's view stack is read only to compare identities)
  // the fixture goes in with the debug module (the log commands exist only while it is loaded) in one selection and one restart: a second restart would count a second
  // unstable boot, and the boot after it would be a safe-mode boot (the harness allows one controlled install restart)
  await installWithDebug(rt, 'test/ui-views');
  const events = await rt.listen();
  const opened = (flags: number) => events.until('VIEW_OPENED', (e) => (e.args[1]! | 0) === flags, { withinMs: 10_000 });
  const viewClosed = () => events.until('VIEW_CLOSED', undefined, { withinMs: 10_000 });
  const inputOpened = () => events.until('TEXT_INPUT_OPENED', undefined, { withinMs: 10_000 });
  const sameStack = (want: number[], message: string) =>
    pollEmulated(session, () => fw.vptrs(), (v) => v.length === want.length && v.every((x, i) => x === want[i]), { emulatedMs: 10_000, message });
  // the main screen is up when the view stack has a view; SETTINGS is open when the top view changed
  await pollEmulated(session, () => fw.vptrs(), (v) => v.length > 0, { emulatedMs: 20_000, message: 'the main screen is not on the view stack' });
  const stockRows = async () => {
    const below = await fw.vptrs();
    await tap('SETTINGS');
    await pollEmulated(session, () => fw.vptrs(), (v) => v.length > below.length, { emulatedMs: 10_000, message: 'SETTINGS did not open' });
    const n = await fw.topMenuItems();
    await leave(rt);
    return n;
  };
  const mainStack = await fw.vptrs();
  expect(RANGES.ui_views_test, 'reserved command range of the fixture').toEqual([0x0f, 0x0f]);
  const first = await report(rt);
  expect([first.row1, first.row2], 'dk_ui_settings_row in init: both rows added').toEqual([0, 0]);
  expect([first.fullOpen, first.overlayOpen]).toEqual([0, 0]);

  // The rows: two more items in the SETTINGS menu, at its end.
  const rowsWith = await stockRows();
  // the stock rows are the rest: there is one restart only, so the menu is never measured without the module, and the firmware keeps the rows of an unloaded module until it rebuilds the list
  const rowsBefore = rowsWith - 2;
  const row1 = rowsBefore;

  // The full-screen view: a new top view above the SETTINGS menu.
  await pressSettingsRow(rt, row1, rowsWith);
  await opened(0);
  const open = await until(rt, (r) => r.drawsFull > 0, 'the view was never drawn');
  expect(open.fullOpen, 'dk_ui_view_open returned a view').toBe(1);
  const withView = await fw.vptrs();
  expect(withView[0], 'the top view is not the SETTINGS menu below it').not.toBe(withView[1]);
  const grown = await until(rt, (r) => r.drawsFull > open.drawsFull, 'the view is drawn on every frame, not once');
  expect(grown.drawsFull).toBeGreaterThan(open.drawsFull);

  // YES in the view opens the stock text input (another top view); NO leaves it without a call of `done`.
  await tap('YES');
  await until(rt, (r) => r.keyPresses === 1, 'the view got no key press');
  // core/ui pushes the naming screen after the release of YES (the screen acts on a release: pushed at once it would take the release of this press as its own YES)
  await inputOpened();
  const withInput = await fw.vptrs();
  expect(withInput.slice(1), 'under it the view and the stack are as before').toEqual(withView);
  expect((await report(rt)).doneCalls, 'the release of the YES that opened the screen did not answer it').toBe(0);
  await tap('NO');
  await sameStack(withView, 'NO took the naming screen off the stack');
  expect((await report(rt)).doneCalls, 'NO on the text input calls nothing').toBe(0);
  expect((await report(rt)).fullOpen, 'the view is still open under the text input').toBe(1);

  // YES on the text input calls `done` with the start text.
  await tap('YES');
  await tap('YES');
  const done = await until(rt, (r) => r.doneCalls === 1, 'YES on the text input did not call done');
  expect(done.doneText, 'done gets the text as edited (the start text, trailing blanks removed)').toBe('AB');
  await sameStack(withView, 'YES took the naming screen off the stack');

  // NO in the view closes it: `closed` runs and the stack is SETTINGS alone again.
  await tap('NO');
  const closed = await until(rt, (r) => r.closedFull === 1, 'NO in the view did not close it');
  expect(closed.fullOpen).toBe(0);
  // the release of that NO went to our view, not to the SETTINGS menu below it: SETTINGS is still there
  await viewClosed();
  await sameStack(withView.slice(1), 'the view left the stack and SETTINGS stayed');

  // The overlay: the second row, a top view, closed by the module.
  await leave(rt);
  await pressSettingsRow(rt, row1 + 1, rowsWith);
  await opened(1);
  const overlay = await until(rt, (r) => r.drawsOverlay > 0, 'the overlay was never drawn');
  expect(overlay.closedOverlay).toBe(0);
  const withOverlay = await fw.vptrs();
  expect(withOverlay[0], 'the overlay is a new top view above SETTINGS').not.toBe(withOverlay[1]);
  expect((await dk.request(REPORT, Uint8Array.of(1))).status, 'the close is queued for tick').toBe(0);
  await until(rt, (r) => r.closedOverlay === 1, 'the overlay did not close');
  await sameStack(withOverlay.slice(1), 'the overlay left the stack');
  await leave(rt);
  expect(await fw.vptrs(), 'back on the main screen').toEqual(mainStack);

  const path = moduleStorePath(device, 'test/ui-views');
  const unload = async (what: string) =>
    expect(await dk.unloadAndWait(path), what).toEqual({ result: 0, unloaded: true });
  const reload = async () => {
    expect(await dk.loadAndWait(path), 'the module loads again').toEqual({ result: 0, unloaded: false });
    expect((await dk.modules()).map((m) => m.name)).toContain('test/ui-views');
  };

  // An unload takes an open view off the stack and removes the rows, without a fault.
  await pressSettingsRow(rt, row1, rowsWith);
  await opened(0);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open before the unload');
  const beforeUnload = await fw.vptrs();
  await unload('the unload of a module that has views, rows and no menu');
  await viewClosed();
  await sameStack(beforeUnload.slice(1), 'only the SETTINGS menu is left under the removed view');
  await session.expectNoFault('the unload closed the view');
  await leave(rt);
  expect((await dk.modules()).map((m) => m.name), 'the module is not loaded').not.toContain('test/ui-views');
  // the rows stay in the SETTINGS list that the firmware has built until its next rebuild (core/ui README, "SETTINGS rows"): the row count is not asserted here, and running such a row is a no-op (below)
  await session.expectNoFault('the SETTINGS menu after the unload');

  // The pool is not stuck: the module loads again and opens a view (a slot of the unloaded one is free).
  await reload();
  expect(await stockRows(), 'the rows are back').toBe(rowsBefore + 2);
  await pressSettingsRow(rt, row1, rowsWith);
  await opened(0);
  await until(rt, (r) => r.fullOpen === 1, 'a view of the reloaded module did not open');
  await tap('NO');
  await viewClosed();
  await until(rt, (r) => r.closedFull === 1, 'the view of the reloaded module did not close');
  await leave(rt);

  // A stock row that was built before the unload does nothing when it is run afterwards.
  await tap('SETTINGS');
  for (let i = 0; i < rowsWith; i++) await tap('UP');
  for (let i = 0; i < row1; i++) await tap('DOWN');
  const settingsOnly = await fw.vptrs();
  await unload('the unload with the SETTINGS menu open');
  await tap('YES');
  expect(await fw.vptrs(), 'YES on the row of an unloaded module opened no view').toEqual(settingsOnly);
  await session.expectNoFault('a stale SETTINGS row');
  await leave(rt);

  // An unload with the text input open: YES calls nothing of the unloaded module, the screen goes, and a text input of the reloaded module works.
  await reload();
  await pressSettingsRow(rt, row1, rowsWith);
  await opened(0);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open for the text input case');
  await tap('YES');
  await until(rt, (r) => r.keyPresses === 1, 'the view got no key press');
  await inputOpened();
  const inputUp = await fw.vptrs();
  await unload('the unload with a view and a text input open');
  await viewClosed();
  await sameStack([inputUp[0]!, ...inputUp.slice(2)], 'the naming screen and SETTINGS are left');
  await tap('YES'); // the naming screen answers YES: its `done` belongs to code that is gone and must not run
  await sameStack(inputUp.slice(2), 'YES closed the naming screen');
  await session.expectNoFault('YES on a text input of an unloaded module');
  await leave(rt);
  await reload();
  await pressSettingsRow(rt, row1, rowsWith);
  await opened(0);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open after the reload');
  await tap('YES');
  await tap('YES');
  const again = await until(rt, (r) => r.doneCalls === 1, 'a text input of the reloaded module did not call done (the input is stuck)');
  expect(again.doneText).toBe('AB');
}

runtimeTests('a content module adds SETTINGS rows, opens a full-screen view and an overlay, takes text input, and an unload closes its view and rows', expectUiViews, {
  dsp: 'zero',
});

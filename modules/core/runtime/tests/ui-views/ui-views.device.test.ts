import { RANGES } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { Device } from '@digi/test-support';
import type { TestRuntime } from '@digi/regression/support.ts';
import { REDRAW } from '@digi/regression/runtime/fx-live.ts';
import { installModule, moduleArtifact, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';

/**
 * What only the firmware shows of the module screens of core/ui (docs/plans/track-manager.md, "core/ui"). The fixture `test/ui-views` is a content module: in `init` it adds two
 * SETTINGS rows (`dk_ui_settings_row`); the first opens a full-screen view (frame and text; YES opens the stock text input, NO closes), the second an overlay.
 * Everything else is a unit test: the pool, the row registry and the canvas mapping (`modules/core/adapters/ui/host/tests`), the observer list and the owner id of a context
 * (`core/runtime` `spans_tests.rs`, `cleanup_tests.rs`).
 *
 * - the rows are in SETTINGS after the stock rows, in load order, and YES on a row runs its function;
 * - the full-screen view draws on every frame and gets the keys; YES opens the stock NameView; YES on it calls `done` with the text, NO calls nothing; NO closes the view and `closed` runs;
 * - the overlay is drawn over the stock screen and goes away on the module's request;
 * - an unload closes the view that is open and removes the rows, without a fault (the module's code is gone, so nothing may call it).
 */
const REPORT = 0x1b;

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

/** The stock SETTINGS list has eight rows; the rows of modules come after them (and after the DIGIKIT row when the image has one). */
const FIRST_MODULE_ROW = 8;

/** Open SETTINGS and press YES on row `row` (counted from the top). */
async function pressSettingsRow(rt: TestRuntime, row: number): Promise<void> {
  const tap = tapOf(rt);
  await tap('SETTINGS');
  for (let i = 0; i < row; i++) await tap('DOWN');
  await tap('YES');
}

/** Back out of every screen to the main screen. */
async function leave(rt: TestRuntime): Promise<void> {
  const tap = tapOf(rt);
  for (let i = 0; i < 4; i++) await tap('NO');
}

/**
 * Press YES on the module rows in turn until `opened(report)`: the row index is found by the module's own state (the first row of this module is the one whose function
 * ran), not by a count that depends on which other modules are in the image. A row that is not ours opens a stock menu, which `leave` closes.
 */
async function openOwnRow(rt: TestRuntime, opened: (r: Report) => boolean, from = FIRST_MODULE_ROW): Promise<number> {
  for (let row = from; row < from + 4; row++) {
    await pressSettingsRow(rt, row);
    const r = await report(rt);
    if (opened(r)) return row;
    await leave(rt);
  }
  throw new Error('none of the SETTINGS rows after the stock rows ran a function of test/ui-views');
}

async function frameHash(rt: TestRuntime): Promise<string> {
  return rt.session.client.frameHash();
}

async function expectUiViews(rt: TestRuntime): Promise<void> {
  const { session, dk } = rt;
  const device = session.options.device as Device;
  await expectRuntimeReady(session);
  await installModule(dk, 'test/ui-views', moduleArtifact(device, 'test/ui-views'), { device, session });
  expect(RANGES.ui_views_test, 'reserved command range of the fixture').toEqual([0x1b, 0x1b]);
  const tap = tapOf(rt);

  const first = await report(rt);
  expect([first.row1, first.row2], 'dk_ui_settings_row in init: both rows added').toEqual([0, 0]);
  expect([first.fullOpen, first.overlayOpen]).toEqual([0, 0]);

  // The full-screen view: the first row of the module.
  const row1 = await openOwnRow(rt, (r) => r.fullOpen === 1);
  const open = await until(rt, (r) => r.drawsFull > 0, 'the view was never drawn');
  expect(open.fullOpen, 'dk_ui_view_open returned a view').toBe(1);
  const viewFrame = await frameHash(rt);
  const grown = await until(rt, (r) => r.drawsFull > open.drawsFull, 'the view is drawn on every frame, not once');
  expect(grown.drawsFull).toBeGreaterThan(open.drawsFull);

  // YES in the view opens the stock text input; NO leaves it without a call of `done`.
  await tap('YES');
  await until(rt, (r) => r.keyPresses === 1, 'the view got no key press');
  const input = await frameHash(rt);
  expect(input, 'the stock naming screen replaced the view on the screen').not.toBe(viewFrame);
  await tap('NO');
  expect((await report(rt)).doneCalls, 'NO on the text input calls nothing').toBe(0);
  expect((await report(rt)).fullOpen, 'the view is still open under the text input').toBe(1);

  // YES on the text input calls `done` with the start text.
  await tap('YES');
  await tap('YES');
  const done = await until(rt, (r) => r.doneCalls === 1, 'YES on the text input did not call done');
  expect(done.doneText, 'done gets the text as edited (the start text, trailing blanks removed)').toBe('AB');

  // NO in the view closes it: `closed` runs.
  await tap('NO');
  const closed = await until(rt, (r) => r.closedFull === 1, 'NO in the view did not close it');
  expect(closed.fullOpen).toBe(0);

  // The overlay: the second row. It is drawn over the screen and closed by the module.
  await leave(rt);
  await openOwnRow(rt, (r) => r.overlayOpen === 1, row1 + 1);
  const overlay = await until(rt, (r) => r.drawsOverlay > 0, 'the overlay was never drawn');
  expect(overlay.closedOverlay).toBe(0);
  expect((await dk.request(REPORT, Uint8Array.of(1))).status, 'the close is queued for tick').toBe(0);
  await until(rt, (r) => r.closedOverlay === 1, 'the overlay did not close');
  await leave(rt);

  // An unload closes an open view and removes the rows, without a fault.
  await openOwnRow(rt, (r) => r.fullOpen === 1, row1);
  const withView = await frameHash(rt);
  const path = moduleStorePath(device, 'test/ui-views');
  expect(await dk.unloadAndWait(path), 'the unload of a module that has views, rows and no menu').toEqual({ result: 0, unloaded: true });
  const gone = await pollEmulated(session, () => frameHash(rt), (h) => h !== withView, {
    emulatedMs: 10_000,
    message: 'the view of the unloaded module is still on the screen',
  });
  expect(gone).not.toBe(withView);
  await session.expectNoFault('the unload closed the view');
  await leave(rt);
  const names = (await dk.modules()).map((m) => m.name);
  expect(names, 'the module is not loaded').not.toContain('test/ui-views');
  // the rows are gone: the list at the module's first row looks as it does for a row that does not exist, and YES on it runs nothing
  await pressSettingsRow(rt, row1);
  await session.expectNoFault('YES on the place of a removed row');
}

runtimeTests('a content module adds SETTINGS rows, opens a full-screen view and an overlay, takes text input, and an unload closes its view and rows', expectUiViews, {
  dsp: 'zero',
});

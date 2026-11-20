import fs from 'node:fs';
import path from 'node:path';
import { RANGES } from '@digi/dk-session/gen_proto';
import { bootWindowFacts, emulatorRead, repoRoot, type Device } from '@digi/test-support';
import { parse } from 'smol-toml';
import { expect } from 'vitest';
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
 * The screen is read from firmware state, never from pixels: the view stack (Brain, view manager, list of nodes, the View of each node and its vptr: the `[ui]` facts of the device
 * files, `bootWindowFacts`) and the item list of the stock SETTINGS menu (the Menu object's item vector; the offsets are the structs of `devices/fw/menu.toml`).
 * - the rows are in SETTINGS after the stock rows (two more items while the module is loaded), and YES on a row runs its function;
 * - the full-screen view is a new top view of the stack (its vptr is not the one below), draws on every frame and gets the keys; YES opens the stock NameView (another top view);
 *   YES on it calls `done` with the text, NO calls nothing; NO closes the view and `closed` runs;
 * - the overlay is a new top view and goes away on the module's request;
 * - an unload with the view open takes it off the stack (the stack is what it was before the view) and removes the rows (the item count is what it was before the module), without a fault.
 */
const REPORT = 0x1b;
/** The elements of a menu's item vector are `shared_ptr<MenuItem>`: two words (core/ui `rows_of`). */
const ITEM_BYTES = 8;
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

/** The offset of field `field` in the struct `struct` of devices/fw/menu.toml. */
function fwField(struct: string, field: string): number {
  const rules = parse(fs.readFileSync(path.join(repoRoot(), 'devices/fw/menu.toml'), 'utf8')) as {
    struct: { name: string; fields: { name: string; off: number }[] }[];
  };
  const f = rules.struct.find((s) => s.name === struct)?.fields.find((x) => x.name === field);
  if (!f) throw new Error(`devices/fw/menu.toml: no ${struct}.${field}`);
  return f.off;
}

/** The firmware's view stack and SETTINGS menu, read from guest memory. */
function firmware(rt: TestRuntime) {
  const device = rt.session.options.device as Device;
  const facts = bootWindowFacts(device);
  if (!facts) throw new Error(`${device} has no [ui] facts`);
  const read = emulatorRead(rt.session);
  const word = async (addr: number) => {
    const b = await read(addr, 4);
    return new DataView(b.buffer, b.byteOffset, b.byteLength).getUint32(0);
  };
  const viewAt = fwField('cf.menu.struct.VerticalMenuView', 'view');
  const itemsBegin = fwField('cf.menu.struct.CompactVerticalMenuView', 'items_begin');
  const itemsEnd = fwField('cf.menu.struct.CompactVerticalMenuView', 'items_end');
  /** The View objects of the stack, the top one first. */
  const views = async (): Promise<number[]> => {
    const brain = await word(facts.brainPtr);
    if (brain === 0) return [];
    const list = brain + facts.viewManagerOff + facts.listOff;
    const out: number[] = [];
    let node = await word(list + facts.listPrevOff);
    while (node !== list && out.length < MAX_STACK) {
      out.push(await word(node + facts.nodeViewOff));
      node = await word(node + facts.listPrevOff);
    }
    return out;
  };
  /** The vtable pointers of the stack, the top one first. */
  const vptrs = async () => Promise.all((await views()).map((v) => word(v)));
  /** How many items the top view's menu has (the top view must be a menu: SETTINGS). */
  const topMenuItems = async (): Promise<number> => {
    const [top] = await views();
    if (top === undefined) throw new Error('the view stack is empty');
    const menu = top - viewAt;
    return ((await word(menu + itemsEnd)) - (await word(menu + itemsBegin))) / ITEM_BYTES;
  };
  return { vptrs, topMenuItems };
}

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

async function expectUiViews(rt: TestRuntime): Promise<void> {
  const { session, dk } = rt;
  const device = session.options.device as Device;
  const fw = firmware(rt);
  await expectRuntimeReady(session);
  const tap = tapOf(rt);
  const stockRows = async () => {
    await tap('SETTINGS');
    const n = await fw.topMenuItems();
    await leave(rt);
    return n;
  };
  const rowsBefore = await stockRows();
  const mainStack = await fw.vptrs();

  await installModule(dk, 'test/ui-views', moduleArtifact(device, 'test/ui-views'), { device, session });
  expect(RANGES.ui_views_test, 'reserved command range of the fixture').toEqual([0x1b, 0x1b]);
  const first = await report(rt);
  expect([first.row1, first.row2], 'dk_ui_settings_row in init: both rows added').toEqual([0, 0]);
  expect([first.fullOpen, first.overlayOpen]).toEqual([0, 0]);

  // The rows: two more items in the SETTINGS menu, at its end.
  const rowsWith = await stockRows();
  expect(rowsWith, 'the SETTINGS menu has the two rows of the module more').toBe(rowsBefore + 2);
  const row1 = rowsWith - 2;

  // The full-screen view: a new top view above the SETTINGS menu.
  await pressSettingsRow(rt, row1);
  const open = await until(rt, (r) => r.drawsFull > 0, 'the view was never drawn');
  expect(open.fullOpen, 'dk_ui_view_open returned a view').toBe(1);
  const withView = await fw.vptrs();
  expect(withView.length, 'the view is one more on the stack than SETTINGS alone').toBe(mainStack.length + 2);
  expect(withView[0], 'the top view is not the SETTINGS menu below it').not.toBe(withView[1]);
  const grown = await until(rt, (r) => r.drawsFull > open.drawsFull, 'the view is drawn on every frame, not once');
  expect(grown.drawsFull).toBeGreaterThan(open.drawsFull);

  // YES in the view opens the stock text input (another top view); NO leaves it without a call of `done`.
  await tap('YES');
  await until(rt, (r) => r.keyPresses === 1, 'the view got no key press');
  const withInput = await fw.vptrs();
  expect(withInput.length, 'the naming screen is one more view').toBe(withView.length + 1);
  expect(withInput.slice(1), 'under it the view and the stack are as before').toEqual(withView);
  await tap('NO');
  expect(await fw.vptrs(), 'NO took the naming screen off the stack').toEqual(withView);
  expect((await report(rt)).doneCalls, 'NO on the text input calls nothing').toBe(0);
  expect((await report(rt)).fullOpen, 'the view is still open under the text input').toBe(1);

  // YES on the text input calls `done` with the start text.
  await tap('YES');
  await tap('YES');
  const done = await until(rt, (r) => r.doneCalls === 1, 'YES on the text input did not call done');
  expect(done.doneText, 'done gets the text as edited (the start text, trailing blanks removed)').toBe('AB');
  expect(await fw.vptrs(), 'YES took the naming screen off the stack').toEqual(withView);

  // NO in the view closes it: `closed` runs and the stack is SETTINGS alone again.
  await tap('NO');
  const closed = await until(rt, (r) => r.closedFull === 1, 'NO in the view did not close it');
  expect(closed.fullOpen).toBe(0);
  expect(await fw.vptrs(), 'the view left the stack').toEqual(withView.slice(1));

  // The overlay: the second row, a top view, closed by the module.
  await leave(rt);
  await pressSettingsRow(rt, row1 + 1);
  const overlay = await until(rt, (r) => r.drawsOverlay > 0, 'the overlay was never drawn');
  expect(overlay.closedOverlay).toBe(0);
  const withOverlay = await fw.vptrs();
  expect(withOverlay.length, 'the overlay is one more view on the stack').toBe(mainStack.length + 2);
  expect((await dk.request(REPORT, Uint8Array.of(1))).status, 'the close is queued for tick').toBe(0);
  await until(rt, (r) => r.closedOverlay === 1, 'the overlay did not close');
  expect((await fw.vptrs()).length, 'the overlay left the stack').toBe(mainStack.length + 1);
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
  await pressSettingsRow(rt, row1);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open before the unload');
  expect((await fw.vptrs()).length).toBe(mainStack.length + 2);
  await unload('the unload of a module that has views, rows and no menu');
  const after = await pollEmulated(session, () => fw.vptrs(), (v) => v.length === mainStack.length + 1, {
    emulatedMs: 10_000,
    message: 'the view of the unloaded module is still on the view stack',
  });
  expect(after, 'only the SETTINGS menu is left under the removed view').toEqual(withView.slice(1));
  await session.expectNoFault('the unload closed the view');
  await leave(rt);
  expect((await dk.modules()).map((m) => m.name), 'the module is not loaded').not.toContain('test/ui-views');
  expect(await stockRows(), 'the rows of the module are gone from the SETTINGS menu').toBe(rowsBefore);
  await session.expectNoFault('the SETTINGS menu after the unload');

  // The pool is not stuck: the module loads again and opens a view (a slot of the unloaded one is free).
  await reload();
  expect((await stockRows()), 'the rows are back').toBe(rowsBefore + 2);
  await pressSettingsRow(rt, row1);
  await until(rt, (r) => r.fullOpen === 1, 'a view of the reloaded module did not open');
  await tap('NO');
  await until(rt, (r) => r.closedFull === 1, 'the view of the reloaded module did not close');
  await leave(rt);

  // A stock row that was built before the unload does nothing when it is run afterwards.
  await tap('SETTINGS');
  for (let i = 0; i < row1; i++) await tap('DOWN');
  const settingsOnly = await fw.vptrs();
  await unload('the unload with the SETTINGS menu open');
  await tap('YES');
  expect(await fw.vptrs(), 'YES on the row of an unloaded module opened no view').toEqual(settingsOnly);
  await session.expectNoFault('a stale SETTINGS row');
  await leave(rt);

  // An unload with the text input open: YES calls nothing of the unloaded module, the screen goes, and a text input of the reloaded module works.
  await reload();
  await pressSettingsRow(rt, row1);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open for the text input case');
  await tap('YES');
  await until(rt, (r) => r.keyPresses === 1, 'the view got no key press');
  const inputUp = await fw.vptrs();
  expect(inputUp.length, 'the naming screen is on the stack').toBe(mainStack.length + 3);
  await unload('the unload with a view and a text input open');
  const left = await pollEmulated(session, () => fw.vptrs(), (v) => v.length === mainStack.length + 2, {
    emulatedMs: 10_000,
    message: 'the view of the unloaded module is still on the view stack under its text input',
  });
  expect(left, 'the naming screen and SETTINGS are left').toEqual([inputUp[0], ...inputUp.slice(2)]);
  await tap('YES'); // the naming screen answers YES: its `done` belongs to code that is gone and must not run
  expect((await fw.vptrs()).length, 'YES closed the naming screen').toBe(mainStack.length + 1);
  await session.expectNoFault('YES on a text input of an unloaded module');
  await leave(rt);
  await reload();
  await pressSettingsRow(rt, row1);
  await until(rt, (r) => r.fullOpen === 1, 'the view did not open after the reload');
  await tap('YES');
  await tap('YES');
  const again = await until(rt, (r) => r.doneCalls === 1, 'a text input of the reloaded module did not call done (the input is stuck)');
  expect(again.doneText).toBe('AB');
}

runtimeTests('a content module adds SETTINGS rows, opens a full-screen view and an overlay, takes text input, and an unload closes its view and rows', expectUiViews, {
  dsp: 'zero',
});

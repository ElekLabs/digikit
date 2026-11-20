import fs from 'node:fs';
import path from 'node:path';
import { emulatorRead, panelInput, pollUntil, readViewStack, repoRoot, viewStackFacts, type Device } from '@digi/test-support';
import { parse } from 'smol-toml';
import { expect, test } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { cardRigs, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { moduleStorePath } from '@digi/regression/runtime/install.ts';
import { decodeSettings, settingsMap, settingsRecord } from '@digi/regression/runtime/project-kit.ts';
import type { TestRuntime } from '@digi/regression/support.ts';

/**
 * The track manager on the device (docs/plans/track-manager.md, phase 4). The module `digikit/track-manager` needs `core/ui` and the `shortcuts` service (its imports say so).
 * No SHARC. The screen is read from the firmware's view stack (`readViewStack`, the `[ui]` facts of the device file), the saved names from the settings record on the card.
 * Every wait is on firmware state or the drained input queue.
 *
 * - SETTINGS > TRACK MANAGER (the last row) opens a full-screen view (a new opaque top view);
 * - [TRIG 10] then [YES] opens the stock naming screen (one more top view, pushed after the release of YES); [UP] edits the first character and [YES] saves: the naming screen leaves
 *   and the settings record holds a name for track 10 (index 9) and no other;
 * - after a restart the saved name is still in the record, and loaded by the module: [TRK]+[YES] shows the overlay, a new top view that is not opaque; releasing YES removes it.
 */
const MODULE = 'digikit/track-manager';
const MAX_STACK = 32;
const ITEM_BYTES = 8; // shared_ptr<MenuItem> (core/ui rows_of)

/** The offset of field `field` in the struct `struct` of devices/fw/menu.toml (as the ui-views fixture test reads it). */
function fwField(struct: string, field: string): number {
  const rules = parse(fs.readFileSync(path.join(repoRoot(), 'devices/fw/menu.toml'), 'utf8')) as {
    struct: { name: string; fields: { name: string; off: number }[] }[];
  };
  const f = rules.struct.find((s) => s.name === struct)?.fields.find((x) => x.name === field);
  if (!f) throw new Error(`devices/fw/menu.toml: no ${struct}.${field}`);
  return f.off;
}

function firmware(rt: TestRuntime, device: Device) {
  const facts = viewStackFacts(device);
  if (!facts) throw new Error(`${device} has no [ui] facts`);
  const read = emulatorRead(rt.session);
  const word = async (addr: number) => {
    const b = await read(addr, 4);
    return new DataView(b.buffer, b.byteOffset, b.byteLength).getUint32(0);
  };
  const viewAt = fwField('cf.menu.struct.VerticalMenuView', 'view');
  const itemsBegin = fwField('cf.menu.struct.CompactVerticalMenuView', 'items_begin');
  const itemsEnd = fwField('cf.menu.struct.CompactVerticalMenuView', 'items_end');
  const stack = () => readViewStack(read, facts, MAX_STACK);
  /** The items of the top view's menu (the top view must be SETTINGS). */
  const topMenuItems = async () => {
    const [top] = await stack();
    if (!top) throw new Error('the view stack is empty');
    const menu = top.view - viewAt;
    return ((await word(menu + itemsEnd)) - (await word(menu + itemsBegin))) / ITEM_BYTES;
  };
  return { stack, topMenuItems };
}

/** The saved names: the value of the module's `names` key (version byte, track count, then a length byte and the characters of each track), or null before it is written. */
async function savedNames(h: CardRig, rt: TestRuntime): Promise<string[] | null> {
  let rec;
  try {
    rec = await h.usb(rt, settingsRecord);
  } catch {
    return null;
  }
  const map = settingsMap(decodeSettings(rec));
  const key = Object.keys(map).find((k) => k.startsWith('digikit/track-manager') && k.endsWith('/names'));
  if (!key) return null;
  const b = Buffer.from(map[key]!, 'hex');
  expect(b[0], 'value version').toBe(1);
  const out: string[] = [];
  let at = 2;
  for (let t = 0; t < b[1]!; t++) {
    const n = b[at]!;
    out.push(b.subarray(at + 1, at + 1 + n).toString('latin1'));
    at += 1 + n;
  }
  return out;
}

const names = (h: CardRig, rt: TestRuntime, done: (n: string[] | null) => boolean, message: string) =>
  pollUntil(rt.clock, () => savedNames(h, rt), done, { withinMs: 20_000, intervalMs: 500, message });

cardRigs('track manager', { dsp: 'zero', initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('the manager screen names a track, the name survives a restart, and TRK+YES shows the overlay while held', async () => {
    let rt = await h.load('track-manager', { modules: [MODULE], selection: [moduleStorePath(h.device, MODULE)] });
    const info = await bootInfo(rt.session, h.plan);
    expect(info.results.slice(0, 1), 'the module loads').toEqual([0]);
    const drained = { drained: true } as const;
    let fw = firmware(rt, h.device);
    const stackOfLength = (length: number, message: string) =>
      pollUntil(rt.clock, () => fw.stack(), (s) => s.length === length, { withinMs: 10_000, intervalMs: 100, message });

    // SETTINGS > TRACK MANAGER: the module's row is the last one
    const main = await fw.stack();
    await rt.ui.tap('SETTINGS', { until: drained });
    const settings = await fw.stack();
    expect(settings.length, 'SETTINGS is one view above the main screen').toBe(main.length + 1);
    const rows = await fw.topMenuItems();
    for (let i = 0; i < rows - 1; i++) await rt.ui.tap('DOWN', { until: drained });
    await rt.ui.tap('YES', { until: drained });
    const manager = await stackOfLength(settings.length + 1, 'the manager screen was not opened');
    expect(manager[0]!.opaque, 'the manager screen is full screen').not.toBe(0);
    expect(manager[0]!.vptr, 'a view of the module, not the SETTINGS menu below it').not.toBe(manager[1]!.vptr);

    // TRIG 10 selects track 10; YES opens the naming screen (pushed after the release of YES)
    await rt.ui.tap('TRIG 10', { until: drained });
    await rt.ui.tap('YES', { until: drained });
    const naming = await stackOfLength(manager.length + 1, 'the naming screen was not pushed');
    expect(naming.slice(1), 'under it the manager screen and the stack as before').toEqual(manager);

    // UP edits the character under the cursor; YES saves and closes the naming screen
    await rt.ui.tap('UP', { until: drained });
    await rt.ui.tap('YES', { until: drained });
    await stackOfLength(manager.length, 'YES on the naming screen did not close it');
    const saved = await names(h, rt, (n) => n !== null && n.some((x) => x !== ''), 'the settings record holds the name');
    expect(saved!.length, 'one entry for each track').toBe(16);
    expect(saved!.map((x, i) => (x === '' ? -1 : i)).filter((i) => i >= 0), 'only track 10 has a name').toEqual([9]);
    const name = saved![9]!;
    expect(name.length, 'the edit gave a name').toBeGreaterThan(0);

    // a restart: the name is still on the card
    rt = await h.restart(rt);
    fw = firmware(rt, h.device);
    expect(await names(h, rt, (n) => n !== null, 'the record after the restart'), 'the name survived').toEqual(saved);

    // TRK held pushes the stock track selection; YES under it opens the overlay, a top view that is not opaque, and releasing YES removes it
    const input = panelInput(rt.session, rt.clock);
    await input.button('TRK', true);
    try {
      const held = await fw.stack();
      await input.button('YES', true);
      const overlay = await pollUntil(rt.clock, () => fw.stack(), (s) => s.length === held.length + 1, {
        withinMs: 10_000,
        intervalMs: 100,
        message: 'the overlay was not opened while TRK and YES are held',
      });
      expect(overlay[0]!.opaque, 'the overlay is not opaque').toBe(0);
      expect(overlay.slice(1), 'the stock views below are as they were').toEqual(held);
      await input.button('YES', false);
      await pollUntil(rt.clock, () => fw.stack(), (s) => s.length === held.length, {
        withinMs: 10_000,
        intervalMs: 100,
        message: 'the overlay stayed after YES was released',
      });
    } finally {
      await input.button('YES', false).catch(() => {});
      await input.button('TRK', false);
    }
    await rt.session.expectNoFault('track manager');
  });
});

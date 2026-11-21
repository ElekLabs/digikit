import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { moduleStorePathOf } from '@digi/test-support';
import { cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { logEvents, RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';
import { planSymbol, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { packSynth } from '@digi/regression/runtime/synth.ts';

/**
 * SETTINGS > DIGIKIT > MODULES applies a change without a restart (docs/plans/extension-model.md, E5c-2). Two code-only modules (`a`, `b`) are on the card;
 * the boot selection is `a`. The steps, each after YES on the confirm "APPLY n CHANGE(S)?" (text "NO RESTART"):
 * - tick `b`: the runtime loads it (LOADS says ACTIVE), the boot did not run again (the epoch and the BOOT_START events are those of the boot), the list shows `b` checked;
 * - untick `a`: the runtime unloads it (LOADS says UNLOADED), still the same boot;
 * - the transport plays, tick `a`: the dialog "A NOT APPLIED" with the text STOP PLAYBACK, then "RESTART TO APPLY?"; NO leaves the device running, `a` is not loaded;
 * - with the transport still playing, tick `c` the same way and YES on "RESTART TO APPLY?": the device restarts (the epoch rises).
 * The host cases of the batch (order, texts, refusals, the settings cache, the boot results by index) are in runtime/platform/ui/host/tests/menu.rs.
 * Run only with the go of the owner.
 */

const E_RELOAD_PLAYING = loadCode('E_RELOAD_PLAYING');

/** TICK and LOADED are also names of runtime statics: take the menu's. */
function menuSymbol(plan: RuntimePlan, name: string): number {
  const key = Object.keys(plan.symbols).find((k) => k.startsWith('core/digikit-menu@') && k.endsWith(`:${name}`));
  if (key === undefined) throw new Error(`no menu symbol ${name}`);
  return plan.symbols[key]!;
}

const SYMBOLS = ['DK_LOAD', 'DK_LOAD_N', 'DK_POPUP', 'DK_POPUP_TEXT', 'DK_POPUPS', ...RING_SYMBOLS];

cardRigs('MODULES applies live', { roots: [...RT, 'core/digikit-menu'], symbols: SYMBOLS }, (rig) => {
  test('tick loads and untick unloads without a boot; a playing transport refuses; YES restarts', async () => {
    await rig.first();
    const dir = path.join(rig.artifacts, 'apply-live');
    const names = ['a', 'b', 'c'];
    const files = await Promise.all(names.map((k) => packSynth(rig.device, { name: `live/${k}`, pad: 64 }, dir, `${k}.dkm`)));
    // `modules/<kind>/<k>.dkm`: the kind folder comes from the packed file's records (the store rule of Transfer)
    const storePath = names.map((k, i) => moduleStorePathOf(`live/${k}`, fs.readFileSync(files[i]!), k));
    const rt = await rig.load('apply-live', {
      modules: names.map((_, i) => ({ name: storePath[i]!, file: files[i]! })),
      selection: [storePath[0]!],
    });
    const { session, ui } = rt;
    const peek = async (symbol: string) => (await session.client.memPeek(planSymbol(rig.plan, symbol)))! >>> 0;
    const text = async (symbol: string, i = 0, stride = 24) => {
      const bytes = Buffer.alloc(stride);
      for (let k = 0; k < stride; k += 4)
        bytes.writeUInt32BE((await session.client.memPeek(planSymbol(rig.plan, symbol) + stride * i + k))! >>> 0, k);
      return bytes.toString('latin1').split('\0')[0]!;
    };
    const dialog = async () => [await text('DK_POPUP', 0, 48), await text('DK_POPUP_TEXT', 0, 48)];
    /** TICK or LOADED of the menu: one byte for each row (a), b, c. The check mark itself is not row text. */
    const flags = async (name: 'TICK' | 'LOADED') => {
      const v = (await session.client.memPeek(menuSymbol(rig.plan, name)))! >>> 0;
      return names.map((_, k) => (v >>> (24 - 8 * k)) & 0xff);
    };
    const flagsText = (v: number[]) => v.join();
    const state = async (file: string) => (await rig.usb(rt, (dk) => dk.loadStates())).find((m) => m.name === file)?.state;
    // the runtime log: over SysEx with the debug module, else the runtime's own ring of 32 events; a BOOT_START newer than seq0 is a boot
    const log = () => rig.usb(rt, (dk) => logEvents({ ...rt, dk }, rig.plan));
    const bootStartsAfter = async (seq: number) => (await log()).filter((e) => e.kind === LOG_KIND.BOOT_START && e.seq > seq).length;
    const epoch = async () => Number((await session.status()).epoch);
    const drained = { drained: true } as const;
    const press = (key: string) => ui.tap(key, { until: drained });
    const popups = () => peek('DK_POPUPS');

    const epoch0 = await epoch();
    const seq0 = Math.max(0, ...(await log()).map((e) => e.seq));
    expect(await state('a.dkm'), 'the boot loaded a').toBe('ACTIVE');

    /** SETTINGS > DIGIKIT > MODULES (the fourth row), the cursor on the first row. */
    const openModules = async () => {
      await openDigikitMenu(rt);
      for (let i = 0; i < 3; i++) await press('DOWN');
      await ui.tap('YES', { until: { state: () => peek('DK_LOAD_N'), done: (n) => n === 3, withinMs: 60_000, what: 'MODULES rows' } });
    };
    /** YES on row `row` (the cursor starts on row 0), ending when TICK is `ticked`; then NO: the question. */
    const changeRow = async (row: number, ticked: number[]) => {
      for (let i = 0; i < row; i++) await press('DOWN');
      await ui.tap('YES', { until: { state: () => flags('TICK'), done: (v: number[]) => flagsText(v) === flagsText(ticked), what: 'the row is checked or unchecked' } });
      for (let i = 0; i < row; i++) await press('UP');
      const before = await popups();
      await ui.tap('NO', { until: { state: popups, done: (v) => v === before + 1, what: 'APPLY n CHANGE? opened' } });
    };

    // tick b: loaded live
    await openModules();
    await changeRow(1, [1, 1, 0]);
    expect(await dialog(), 'the confirm of a live change').toEqual(['APPLY 1 CHANGE?', 'NO RESTART']);
    await press('YES');
    await ui.wait({ state: () => state('b.dkm'), done: (s) => s === 'ACTIVE', withinMs: 60_000, what: 'b loaded' });
    expect(await epoch(), 'no new boot').toBe(epoch0);
    expect(await bootStartsAfter(seq0), 'no BOOT_START after the first').toBe(0);

    // untick a: unloaded live (the list closed itself when the batch ended; the cursor is on MODULES)
    await ui.tap('YES', {
      until: {
        state: async () => [await flags('TICK'), await flags('LOADED')],
        done: ([t, l]: number[][]) => flagsText(t!) === '1,1,0' && flagsText(l!) === '1,1,0',
        withinMs: 60_000,
        what: 'MODULES scanned again: a and b checked and loaded',
      },
    });
    await changeRow(0, [0, 1, 0]);
    expect(await dialog()).toEqual(['APPLY 1 CHANGE?', 'NO RESTART']);
    await press('YES');
    await ui.wait({ state: () => state('a.dkm'), done: (s) => s === 'UNLOADED', withinMs: 60_000, what: 'a unloaded' });
    expect(await epoch(), 'no new boot').toBe(epoch0);
    expect(await bootStartsAfter(seq0), 'no BOOT_START after the first').toBe(0);

    // the transport plays: tick a is refused, then the restart question; NO leaves the device running
    await press('NO');
    await press('NO');
    await press('NO');
    await ui.tap('PLAY', { until: drained });
    // the transport state has no address to read: a reload of b (loaded, no menus) is refused while it plays, and harmless while it does not
    await ui.wait({
      state: async () => (await rig.usb(rt, (dk) => dk.reloadAndWait(storePath[1]!))).result,
      done: (r) => r === E_RELOAD_PLAYING,
      withinMs: 30_000,
      what: 'the transport plays',
    });
    await openModules();
    await changeRow(0, [1, 1, 0]);
    await press('YES');
    await ui.wait({ state: dialog, done: (d) => d[0] === 'A NOT APPLIED', withinMs: 60_000, what: 'the refusal dialog' });
    expect(await dialog()).toEqual(['A NOT APPLIED', 'STOP PLAYBACK']);
    await press('YES'); // the notice, then the question
    await ui.wait({ state: dialog, done: (d) => d[0] === 'RESTART TO APPLY?', what: 'the restart question' });
    expect(await dialog()).toEqual(['RESTART TO APPLY?', 'THE CHANGE IS SAVED']);
    await press('NO');
    expect(await state('a.dkm'), 'a stays unloaded').toBe('UNLOADED');
    expect(await epoch(), 'no restart').toBe(epoch0);

    // c, with the transport still playing: the same refusal, and YES on the question restarts the device (the list stayed open)
    await changeRow(2, [1, 1, 1]);
    await press('YES');
    await ui.wait({ state: dialog, done: (d) => d[0] === 'C NOT APPLIED', withinMs: 60_000, what: 'the refusal dialog' });
    await press('YES');
    await ui.wait({ state: dialog, done: (d) => d[0] === 'RESTART TO APPLY?', what: 'the restart question' });
    await ui.tap('YES', { until: { state: epoch, done: (e) => e > epoch0, withinMs: 120_000, what: 'the device restarted' } });
  });
});

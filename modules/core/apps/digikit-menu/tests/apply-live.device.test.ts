import path from 'node:path';
import { expect, test } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { openDigikitMenu } from '@digi/regression/runtime/flows/menu.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { packSynth } from '@digi/regression/runtime/synth.ts';

/**
 * SETTINGS > DIGIKIT > MODULES applies a change without a restart (docs/plans/extension-model.md, E5c-2). Two code-only modules (`a`, `b`) are on the card;
 * the boot selection is `a`. The steps, each after YES on the confirm "APPLY n CHANGE(S)?" (text "NO RESTART"):
 * - tick `b`: the runtime loads it (LOADS says ACTIVE), the boot did not run again (the epoch and the BOOT_START events are those of the boot), the list shows `b` checked;
 * - untick `a`: the runtime unloads it (LOADS says UNLOADED), still the same boot;
 * - the transport plays, tick `a`: the dialog "A NOT APPLIED" with the text STOP PLAYBACK, then "RESTART TO APPLY?"; NO leaves the device running, `a` is not loaded;
 * - with the transport still playing, tick `c` the same way and YES on "RESTART TO APPLY?": the device restarts (the epoch rises).
 * The host cases of the batch (order, texts, refusals, the settings cache, the boot results by index) are in modules/core/adapters/ui/host/tests/menu.rs.
 * Run only with the go of the owner.
 */

const SYMBOLS = ['DK_LOAD', 'DK_LOAD_N', 'DK_POPUP', 'DK_POPUP_TEXT', 'DK_POPUPS'];

cardRigs('MODULES applies live', { roots: [...RT, 'core/digikit-menu'], symbols: SYMBOLS }, (rig) => {
  test('tick loads and untick unloads without a boot; a playing transport refuses; YES restarts', async () => {
    await rig.first();
    const dir = path.join(rig.artifacts, 'apply-live');
    const names = ['a', 'b', 'c'];
    const files = await Promise.all(names.map((k) => packSynth(rig.device, { name: `live/${k}`, pad: 64 }, dir, `${k}.dkm`)));
    const rt = await rig.load('apply-live', {
      modules: names.map((k, i) => ({ name: `modules/${k}.dkm`, file: files[i]! })),
      selection: ['modules/a.dkm'],
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
    const rows = async () => Promise.all(Array.from({ length: await peek('DK_LOAD_N') }, (_, i) => text('DK_LOAD', i)));
    const state = async (file: string) => (await rig.usb(rt, (dk) => dk.loadStates())).find((m) => m.name === file)?.state;
    const bootStarts = async () => (await rig.usb(rt, (dk) => dk.readLog())).filter((e) => e.kind === LOG_KIND.BOOT_START).length;
    const epoch = async () => Number((await session.status()).epoch);
    const drained = { drained: true } as const;
    const press = (key: string) => ui.tap(key, { until: drained });
    const popups = () => peek('DK_POPUPS');

    const epoch0 = await epoch();
    const starts0 = await bootStarts();
    expect(await state('a.dkm'), 'the boot loaded a').toBe('ACTIVE');

    /** SETTINGS > DIGIKIT > MODULES (the fourth row), the cursor on the first row. */
    const openModules = async () => {
      await openDigikitMenu(rt);
      for (let i = 0; i < 3; i++) await press('DOWN');
      await ui.tap('YES', { until: { state: () => peek('DK_LOAD_N'), done: (n) => n === 3, withinMs: 60_000, what: 'MODULES rows' } });
    };
    /** YES on row `row` (the cursor starts on row 0), then NO: the question. */
    const changeRow = async (row: number) => {
      for (let i = 0; i < row; i++) await press('DOWN');
      await ui.tap('YES', { until: { ms: 300, why: 'the check mark has no state row' } });
      for (let i = 0; i < row; i++) await press('UP');
      const before = await popups();
      await ui.tap('NO', { until: { state: popups, done: (v) => v === before + 1, what: 'APPLY n CHANGE? opened' } });
    };

    // tick b: loaded live
    await openModules();
    await changeRow(1);
    expect(await dialog(), 'the confirm of a live change').toEqual(['APPLY 1 CHANGE?', 'NO RESTART']);
    await press('YES');
    await ui.wait({ state: () => state('b.dkm'), done: (s) => s === 'ACTIVE', withinMs: 60_000, what: 'b loaded' });
    expect(await epoch(), 'no new boot').toBe(epoch0);
    expect(await bootStarts(), 'no BOOT_START after the first').toBe(starts0);

    // untick a: unloaded live (the list closed itself when the batch ended; the cursor is on MODULES)
    await ui.tap('YES', {
      until: { state: rows, done: (r: string[]) => r.length === 3 && r[1]!.startsWith('* '), withinMs: 60_000, what: 'MODULES rows with b checked' },
    });
    expect((await rows()).map((r) => r.slice(0, 2)), 'the list shows b checked, a not').toEqual(['  ', '* ', '  ']);
    await changeRow(0);
    expect(await dialog()).toEqual(['APPLY 1 CHANGE?', 'NO RESTART']);
    await press('YES');
    await ui.wait({ state: () => state('a.dkm'), done: (s) => s === 'UNLOADED', withinMs: 60_000, what: 'a unloaded' });
    expect(await epoch(), 'no new boot').toBe(epoch0);
    expect(await bootStarts(), 'no BOOT_START after the first').toBe(starts0);

    // the transport plays: tick a is refused, then the restart question; NO leaves the device running
    await press('NO');
    await press('NO');
    await press('NO');
    await ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
    await openModules();
    await changeRow(0);
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
    await changeRow(2);
    await press('YES');
    await ui.wait({ state: dialog, done: (d) => d[0] === 'C NOT APPLIED', withinMs: 60_000, what: 'the refusal dialog' });
    await press('YES');
    await ui.wait({ state: dialog, done: (d) => d[0] === 'RESTART TO APPLY?', what: 'the restart question' });
    await ui.tap('YES', { until: { state: epoch, done: (e) => e > epoch0, withinMs: 120_000, what: 'the device restarted' } });
  });
});

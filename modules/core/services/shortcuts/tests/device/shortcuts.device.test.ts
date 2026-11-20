import { emulatorRead, readViewStack, viewStackFacts } from '@digi/test-support';
import { expect, test } from 'vitest';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { moduleStorePath } from '@digi/regression/runtime/install.ts';
import type { CardRig } from '@digi/regression/runtime/card-rig.ts';
import type { RuntimeEvent } from '@digi/dk-session';
import type { TestRuntime } from '@digi/regression/support.ts';

/**
 * The `shortcuts` service on the device (docs/plans/track-manager.md, phase 3): the key hook at the entry of the firmware's key walker.
 * The fixture `test/shortcut-demo` binds [TRK]+[YES]; its `on` logs "on" with the selected track as the value (`dk_selected_track`), its `off` logs "off" (error-level MOD_LOG events).
 * No SHARC: the keys are ColdFire work. Every wait is on the log events or on the drained input queue; "no event" is checked with a bounded wait in emulated time.
 * The screen is read from the firmware's view stack (`readViewStack`, the `[ui]` facts of the device file), never from pixels.
 *
 * One boot of the runtime with the fixture, with SETTINGS open (YES on its first row opens a submenu, a visible effect of a YES that reaches the view):
 * - [TRK]+[YES]: `on` runs once, the view stack is what it was (YES did not reach SETTINGS), and the release of YES calls `off` once;
 * - [YES] alone passes: no call, and the stack grew (the submenu opened);
 * - on the main screen [TRK]+[TRIG 3] still selects track 3 (a trig key under [TRK] passes): the next [TRK]+[YES] logs track 2 (0-based).
 */
const FIXTURE = 'test/shortcut-demo';
const WORD_ON = 0x6f6e0000; // "on\0\0": the first word of the MOD_LOG message
const WORD_OFF = 0x6f666600; // "off\0"
const DRAINED = { drained: true } as const;

const isCall = (word: number) => (e: RuntimeEvent) => (e.args[0]! | 0) === word;

async function boot(h: CardRig): Promise<TestRuntime> {
  const rt = await h.load('shortcut-demo', { modules: [FIXTURE], selection: [moduleStorePath(h.device, FIXTURE)] });
  const info = await bootInfo(rt.session, h.plan);
  expect(info.results.slice(0, 1), 'the fixture loads, so shortcuts.dks accepted its record').toEqual([0]);
  return rt;
}

cardRigs('shortcuts: the key hook', { dsp: 'zero', initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('TRK+YES calls on once and off on release and swallows YES; YES alone passes; a trig key under TRK still selects its track', async () => {
    const rt = await boot(h);
    const events = await rt.listen();
    const facts = viewStackFacts(h.device);
    if (!facts) throw new Error(`${h.device} has no [ui] facts`);
    const read = emulatorRead(rt.session);
    const stack = () => readViewStack(read, facts);
    const anyCall = (e: RuntimeEvent) => isCall(WORD_ON)(e) || isCall(WORD_OFF)(e);
    const none = async (why: string) =>
      expect(events.until('MOD_LOG', anyCall, { withinMs: 300 }), why).rejects.toThrow();

    // SETTINGS is the top view
    await rt.ui.tap('SETTINGS', { until: DRAINED });
    const settings = await stack();
    expect(settings.length, 'SETTINGS is on the stack').toBeGreaterThan(0);

    // TRK+YES: `on` runs; the stack is as it was, so SETTINGS did not get YES (the same views, the same top view); the release of YES (part of the tap) calls `off`
    await rt.ui.tap('YES', { until: { event: 'MOD_LOG', match: isCall(WORD_ON), withinMs: 3000 }, held: 'TRK' });
    await rt.ui.wait(DRAINED);
    expect(await stack(), 'the stock views did not get YES: the stack is what it was').toEqual(settings);
    await events.until('MOD_LOG', isCall(WORD_OFF), { withinMs: 3000 });
    await none('one on and one off, no more');

    // YES alone is not bound: it passes to SETTINGS, which opens a submenu (a new top view), and the service calls nothing
    await rt.ui.tap('YES', { until: DRAINED });
    const opened = await stack();
    expect(opened.length, 'YES alone opened a submenu above SETTINGS').toBeGreaterThan(settings.length);
    await none('YES alone calls nothing');
    await rt.ui.tap('NO', { until: DRAINED });
    await rt.ui.tap('NO', { until: DRAINED });

    // TRK+TRIG 3 selects track 3 as stock; the next TRK+YES logs the selected track (0-based)
    await rt.ui.tap('TRIG 3', { until: DRAINED, held: 'TRK' });
    await rt.ui.tap('YES', { until: DRAINED, held: 'TRK' });
    const on = await events.until('MOD_LOG', isCall(WORD_ON), { withinMs: 3000 });
    expect(on.args[4]! | 0, 'the selected track when the combination completed').toBe(2);
    await events.until('MOD_LOG', isCall(WORD_OFF), { withinMs: 3000 });
    await rt.session.expectNoFault('shortcut hook');
  });
});

import { emulatorRead, pollUntil, readViewStack, viewStackFacts } from '@digi/test-support';
import { expect, test } from 'vitest';
import type { DkLogEvent } from '@digi/dk-session';
import { CARD_SYMBOLS, cardRigs, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { moduleStorePath } from '@digi/regression/runtime/install.ts';
import { ringWatch } from '@digi/regression/runtime/runtime-menu.ts';
import type { TestRuntime } from '@digi/regression/support.ts';

/**
 * The `shortcuts` service on the device (docs/plans/track-manager.md, phase 3): the key hook at the entry of the firmware's key walker.
 * Only what the firmware shows is asserted here; the swallow and release rules are unit tests of the crate (`tests/state.rs`).
 * The fixture `test/shortcut-demo` binds [TRK]+[YES]; its `on` logs "on" with the selected track as the value (`dk_selected_track`), its `off` logs "off" (error-level MOD_LOG events).
 * No SHARC. Waits are on log events, on the drained input queue and on the identity of the top view of the firmware's view stack (`readViewStack`); never a count of views.
 *
 * One boot with the fixture, with SETTINGS open (YES on its first row opens a submenu):
 * - [TRK]+[YES] calls `on` and `off`, and SETTINGS stays the top view: YES did not reach it;
 * - [YES] alone passes to SETTINGS (a submenu opens) and calls nothing;
 * - on the main screen [TRK]+[TRIG 3] still selects track 3: the next [TRK]+[YES] logs track 2 (0-based).
 */
const FIXTURE = 'test/shortcut-demo';
const WORD_ON = 0x6f6e0000; // "on\0\0": the first word of the MOD_LOG message
const WORD_OFF = 0x6f666600; // "off\0"
const DRAINED = { drained: true } as const;
const WITHIN = { withinMs: 10_000, intervalMs: 100 } as const;

const isCall = (word: number) => (e: DkLogEvent) => (e.args[0]! | 0) === word;

async function boot(h: CardRig): Promise<TestRuntime> {
  const rt = await h.load('shortcut-demo', { modules: [FIXTURE], selection: [moduleStorePath(h.device, FIXTURE)] });
  const info = await bootInfo(rt.session, h.plan);
  expect(info.results.slice(0, 1), 'the fixture loads, so shortcuts.dks accepted its record').toEqual([0]);
  return rt;
}

cardRigs('shortcuts: the key hook', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('TRK+YES calls on and off and SETTINGS does not get the YES; YES alone does; a trig key under TRK still selects its track', async () => {
    console.info('STEP boot');
    const rt = await boot(h);
    console.info('STEP events');
    // a card has no debug module, so no log commands: the events are read from the runtime's small ring in memory (error-level events, which the fixture and core/ui log)
    const events = await ringWatch(rt, h.plan);
    console.info('STEP facts');
    const facts = viewStackFacts(h.device);
    if (!facts) throw new Error(`${h.device} has no [ui] facts`);
    const read = emulatorRead(rt.session);
    /** The top view of the stack: its object and class. */
    const top = async () => (await readViewStack(read, facts))[0];
    const topIs = (view: number | undefined) => (v: Awaited<ReturnType<typeof top>>) => v?.view === view;
    const anyCall = (e: DkLogEvent) => isCall(WORD_ON)(e) || isCall(WORD_OFF)(e);
    const none = (why: string) => expect(events.none('MOD_LOG', anyCall), why).resolves.toBeUndefined();

    console.info('STEP settings');
    // SETTINGS is open when the top view is not the main screen's
    const main = await top();
    await rt.ui.tap('SETTINGS', { until: DRAINED });
    const settings = await pollUntil(rt.clock, top, (v) => v !== undefined && v.view !== main?.view, {
      ...WITHIN,
      message: 'SETTINGS did not open',
    });

    console.info('STEP trkyes');
    // TRK+YES: `on` and `off` run; when both keys are up the top view is the same SETTINGS view (the stock track selection that TRK pushed is gone again)
    await rt.ui.tap('YES', { until: DRAINED, held: 'TRK' });
    await events.next('MOD_LOG', isCall(WORD_ON), 3000);
    await events.next('MOD_LOG', isCall(WORD_OFF), 3000);
    await none('one on and one off, no more');
    await pollUntil(rt.clock, top, topIs(settings!.view), { ...WITHIN, message: 'SETTINGS is not the top view again' });
    expect(await top(), 'YES did not reach SETTINGS: no submenu opened').toEqual(settings);

    // YES alone is not bound: it reaches SETTINGS, which opens a submenu (another top view), and the service calls nothing
    await rt.ui.tap('YES', { until: DRAINED });
    await pollUntil(rt.clock, top, (v) => v !== undefined && v.view !== settings!.view, { ...WITHIN, message: 'YES alone did not open the submenu' });
    await none('YES alone calls nothing');
    await rt.ui.tap('NO', { until: DRAINED });
    await pollUntil(rt.clock, top, topIs(settings!.view), { ...WITHIN, message: 'NO did not close the submenu' });
    await rt.ui.tap('NO', { until: DRAINED });
    await pollUntil(rt.clock, top, topIs(main?.view), { ...WITHIN, message: 'NO did not leave SETTINGS' });

    // TRK+TRIG 3 selects track 3 as stock; the next TRK+YES logs the selected track (0-based)
    await rt.ui.tap('TRIG 3', { until: DRAINED, held: 'TRK' });
    await rt.ui.tap('YES', { until: DRAINED, held: 'TRK' });
    const on = await events.next('MOD_LOG', isCall(WORD_ON), 3000);
    expect(on.args[4]! | 0, 'the selected track when the combination completed').toBe(2);
    await events.next('MOD_LOG', isCall(WORD_OFF), 3000);
    await rt.session.expectNoFault('shortcut hook');
  });
});

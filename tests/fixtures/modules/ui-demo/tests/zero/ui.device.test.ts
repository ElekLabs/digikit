import { encoder, emulatorRead, readTopMenuItems, readViewStack, viewStackFacts, type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { REDRAW, SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { shot } from '@digi/regression/runtime/fx-session.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';

/**
 * What only the firmware shows of the controls of core/ui (docs/TESTING.md, fixture `test/ui-demo`): a content module that adds the SETTINGS row UI DEMO in `init`
 * (`dk_ui_settings_row`) and opens a menu with one row of each kind, a text page, a list with check marks and a side panel, an empty list, a confirm, a choice and a
 * toast. The row is the last of SETTINGS; the module's state (the value, the check marks, the events and the last answer) is read with its command 0x0f.
 */
const REPORT = 0x0f;

type Demo = { value: number; checks: number[]; events: number; answer: number };

runtimeTests(
  'SDK UI without DSP',
  async (rt) => {
    const { session, dk } = rt;
    const device = session.options.device as Device;
    const artifacts = liveArtifacts(session, 'sdk-ui-zero');
    await installModule(dk, 'test/ui-demo', moduleArtifact(device, 'test/ui-demo'), { device, session });
    expect((await session.status()).dsp).toBe('zero');
    expect((await session.status()).audio).toBeNull();
    const state = async (): Promise<Demo> => {
      const reply = await dk.request(REPORT);
      expect(reply.status, 'the module registered its command in init').toBe(0);
      const b = Buffer.from(reply.body);
      expect(b.length, 'report length').toBe(24);
      return {
        value: b.readInt32BE(0),
        checks: [b.readInt32BE(4), b.readInt32BE(8), b.readInt32BE(12)],
        events: b.readInt32BE(16),
        answer: b.readInt32BE(20),
      };
    };
    const read = async (name: 'value' | 'events' | 'answer' | 'check1' | 'check2') =>
      name === 'check1' ? (await state()).checks[1]! : name === 'check2' ? (await state()).checks[2]! : (await state())[name];
    const press = (key: string, settle = 300) =>
      rt.ui.tap(key, { until: settle ? { ms: settle, why: REDRAW } : { drained: true } });
    const frame = (tag: string) => shot(session, artifacts, tag);
    let row = 0;
    const go = async (target: number) => {
      while (row !== target) {
        await press(row < target ? 'DOWN' : 'UP');
        row += row < target ? 1 : -1;
      }
    };

    // SETTINGS: the module's row is the last (the item count is read from the Menu object of the firmware)
    const facts = viewStackFacts(device);
    if (!facts) throw new Error(`${device} has no [ui] facts`);
    const readMem = emulatorRead(session);
    const depth = async () => (await readViewStack(readMem, facts, 32)).length;
    const main = await pollEmulated(session, depth, (n) => n > 0, {
      emulatedMs: 30_000,
      message: 'the main screen is not on the view stack',
      retry: true,
    });
    await rt.ui.tap('SETTINGS', { until: { state: depth, done: (n: number) => n > main, withinMs: 10_000, what: 'SETTINGS on the view stack' } });
    const rows = await readTopMenuItems(readMem, facts);
    for (let i = 0; i < rows; i++) await press('UP', 0);
    for (let i = 0; i < rows - 1; i++) await press('DOWN', 0);
    await press('YES');
    const initial = await frame('menu');
    const events = await read('events');
    await press('YES');
    expect(await read('events')).toBe(events + 1);
    expect(await frame('toggle-off')).not.toBe(initial);
    await press('YES');
    expect(await frame('toggle-on')).toBe(initial);
    await go(1);
    const value = await read('value');
    const valueFrame = await frame('value-before');
    // The firmware accelerates encoder events; an isolated event can round to zero.
    for (let i = 0; i < 30; i++) {
      await session.client.inputTurn(encoder('ENC A'), 1);
      await rt.ui.wait({ ms: 2, why: SETTLE });
    }
    await rt.ui.wait({ ms: 300, why: SETTLE });
    expect(await read('value')).toBeGreaterThan(value);
    expect(await frame('value-after')).not.toBe(valueFrame);

    await go(5);
    await press('YES');
    const page = await frame('text-page');
    // Text pages hide the cursor; move past the visible rows to exercise scrolling.
    for (let i = 0; i < 7; i++) await press('DOWN');
    expect(await frame('text-page-scrolled')).not.toBe(page);
    await press('NO');
    await go(6);
    await press('YES');
    await press('DOWN');
    const unchecked = await frame('list-unchecked');
    await press('YES');
    expect(await read('check1')).toBe(1);
    expect(await frame('list-checked')).not.toBe(unchecked);
    await press('RIGHT');
    const panel = await frame('list-panel');
    await press('DOWN');
    expect(await frame('panel-second')).not.toBe(panel);
    const beforePanel = await read('events');
    await press('YES');
    expect(await read('events')).toBe(beforePanel + 1);
    await rt.ui.wait({ ms: 2500, why: SETTLE });
    await press('DOWN');
    await press('YES');
    expect(await read('check2')).toBe(1);
    const refused = await frame('list-third-checked');
    const beforeBack = await read('events');
    await press('NO');
    expect(await read('events')).toBe(beforeBack + 1);
    expect(await frame('list-refused-back')).toBe(refused);
    await press('YES');
    await press('NO');
    await go(7);
    await press('YES');
    const empty = await frame('empty-list');
    const beforeEmpty = await read('events');
    await press('YES');
    await press('RIGHT');
    expect(await read('events')).toBe(beforeEmpty);
    expect(await frame('empty-list-no-actions')).toBe(empty);
    await press('NO');

    await go(3);
    await press('YES');
    const confirm = await frame('confirm');
    await press('NO');
    expect(await read('answer')).toBe(0);
    expect(await frame('confirm-dismissed')).not.toBe(confirm);
    await press('YES');
    await press('YES');
    expect(await read('answer')).toBe(1);
    await go(4);
    await press('YES');
    await press('DOWN');
    await press('DOWN');
    await press('YES');
    expect(await read('answer')).toBe(2);
    await press('YES');
    const choice = await frame('choice');
    const beforeCancel = await read('events');
    await press('NO');
    expect(await read('events')).toBe(beforeCancel);
    await press('YES');
    expect(await frame('choice-reopened')).toBe(choice);
    await press('NO');

    await go(2);
    const beforeToast = await read('events');
    await press('YES', 50);
    expect(await read('events')).toBe(beforeToast + 1);
    // the main screen draws the toast (finding 71: a menu does not): back out of the menus, see it, and wait for the firmware to take it off
    await press('NO', 50);
    await press('NO', 50);
    await pollEmulated(session, depth, (n) => n === main, { emulatedMs: 10_000, message: 'the main screen is not back', retry: true });
    const toast = await frame('toast');
    await pollEmulated(session, () => frame('toast-expired'), (hash) => hash !== toast, {
      emulatedMs: 20_000,
      message: 'the toast is still on the main screen',
    });
    await session.expectNoFault('SDK row/list/dialog/toast interactions without DSP');
    expect((await session.status()).audio).toBeNull();
  },
  { dsp: 'zero' },
);

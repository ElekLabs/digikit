import { emulatorRead, panelInput, pollUntil, readTopMenuItems, readViewStack, viewStackFacts, type Device } from '@digi/test-support';
import { expect, test } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import { ringWatch } from '@digi/regression/runtime/runtime-menu.ts';
import { moduleStorePath } from '@digi/regression/runtime/module-artifact.ts';
import { decodeSettings, settingsMap, settingsRecord } from '@digi/regression/runtime/project-kit.ts';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';

/**
 * The track manager on the device (docs/plans/track-manager.md, phase 4). The module `digikit/track-manager` needs `core/ui` and the `shortcuts` service (its imports say so).
 * No SHARC. Waits are on the events core/ui logs for module screens (VIEW_OPENED, VIEW_CLOSED, TEXT_INPUT_OPENED, TEXT_INPUT_DONE) and on firmware state; the view stack is
 * read only to compare the identity of the top view, never to count views. Two tests, one boot each:
 * - a name entered on the manager screen (SETTINGS > TRACK MANAGER, [TRIG 10], [YES], [UP], [YES]) is in the settings record and survives a restart;
 * - [TRK]+[YES] opens the hint (an overlay view) while held, releasing removes it, and [TRK]+[TRIG] under it still selects.
 */
const MODULE = 'digikit/track-manager';
const DRAINED = { drained: true } as const;
const WITHIN = { withinMs: 20_000, intervalMs: 100 } as const;
const OVERLAY = 1;

/** The view stack and the SETTINGS menu, read from guest memory. */
function firmware(rt: TestRuntime, device: Device) {
  const facts = viewStackFacts(device);
  if (!facts) throw new Error(`${device} has no [ui] facts`);
  const read = emulatorRead(rt.session);
  /** The top view of the stack: its object, class and opaque byte. */
  const top = async () => (await readViewStack(read, facts))[0];
  /** The items of the top view's menu (the top view must be SETTINGS). */
  const topMenuItems = () => readTopMenuItems(read, facts);
  return { top, topMenuItems };
}

/** The saved names: the value of the module's `names` key (version byte, track count, then a length byte and the characters of each track), or null before it is written. */
async function savedNames(h: CardRig, rt: TestRuntime): Promise<string[] | null> {
  let rec;
  try {
    rec = await h.usb(rt, settingsRecord);
  } catch {
    return null;
  }
  const map = settingsMap(decodeSettings(rec.raw));
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

/** The module on a card, the events read from the runtime's small ring (a card has no debug module). */
async function boot(h: CardRig, tag: string) {
  const rt = await h.load(tag, { modules: [MODULE], selection: [moduleStorePath(h.device, MODULE)] });
  const info = await bootInfo(rt.session, h.plan);
  expect(info.results.slice(0, 1), 'the module loads').toEqual([0]);
  const events = await ringWatch(rt, h.plan);
  return { rt, events };
}

cardRigs('track manager', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('a name entered on the manager screen is saved and survives a restart', async () => {
    const first = await boot(h, 'track-manager-names');
    const events = first.events;
    let rt: TestRuntime = first.rt;
    let fw = firmware(rt, h.device);
    const viewOpened = (flags: number) => events.next('VIEW_OPENED', (e) => (e.args[1]! | 0) === flags);

    // SETTINGS > TRACK MANAGER: the module's row is the last one; the screen is a full-screen view of the module
    const main = await fw.top();
    await rt.ui.tap('SETTINGS', { until: DRAINED });
    await pollUntil(rt.clock, fw.top, (v) => v !== undefined && v.view !== main?.view, { ...WITHIN, message: 'SETTINGS did not open' });
    const rows = await fw.topMenuItems();
    for (let i = 0; i < rows - 1; i++) await rt.ui.tap('DOWN', { until: DRAINED });
    await rt.ui.tap('YES', { until: DRAINED });
    await viewOpened(0);
    const manager = await fw.top();
    expect(manager?.opaque, 'the manager screen is full screen').not.toBe(0);

    // TRIG 10 selects track 10; YES opens the naming screen (pushed after the release of YES)
    await rt.ui.tap('TRIG 10', { until: DRAINED });
    await rt.ui.tap('YES', { until: DRAINED });
    await events.next('TEXT_INPUT_OPENED');

    // UP edits the character under the cursor; YES saves and closes the naming screen
    await rt.ui.tap('UP', { until: DRAINED });
    await rt.ui.tap('YES', { until: DRAINED });
    await events.next('TEXT_INPUT_DONE');
    await pollUntil(rt.clock, fw.top, (v) => v?.view === manager?.view, { ...WITHIN, message: 'the manager screen is not the top view again' });
    const saved = await names(h, rt, (n) => n !== null && n.some((x) => x !== ''), 'the settings record holds the name');
    expect(saved!.length, 'one entry for each track').toBe(16);
    expect(saved!.map((x, i) => (x === '' ? -1 : i)).filter((i) => i >= 0), 'only track 10 has a name').toEqual([9]);
    expect(saved![9]!.length, 'the edit gave a name').toBeGreaterThan(0);

    // a restart: the name is still on the card
    rt = await h.restart(rt);
    expect(await names(h, rt, (n) => n !== null, 'the record after the restart'), 'the name survived').toEqual(saved);
    await rt.session.expectNoFault('track manager names');
  });

  test('TRK+YES opens the hint while held, releasing removes it, and a trig key under TRK still selects its track', async () => {
    const { rt, events } = await boot(h, 'track-manager-hint');
    const fw = firmware(rt, h.device);
    const input = panelInput(rt.session, rt.clock);

    // TRK pushes the stock track selection: wait until the top view has changed, then that is the view the overlay goes over
    const main = await fw.top();
    await input.button('TRK', true);
    try {
      const selection = await pollUntil(rt.clock, fw.top, (v) => v !== undefined && v.view !== main?.view, { ...WITHIN, message: 'TRK did not push the track selection' });
      await input.button('YES', true);
      await events.next('VIEW_OPENED', (e) => (e.args[1]! | 0) === OVERLAY);
      const overlay = await fw.top();
      expect(overlay?.view, 'the overlay is a new top view over the stock track selection').not.toBe(selection!.view);
      expect(overlay?.opaque, 'the overlay is not opaque').toBe(0);
      await input.button('YES', false);
      await events.next('VIEW_CLOSED');
      await pollUntil(rt.clock, fw.top, (v) => v?.view === selection!.view, { ...WITHIN, message: 'the overlay stayed' });
    } finally {
      await input.button('YES', false).catch(() => {});
      await input.button('TRK', false);
    }
    await rt.session.expectNoFault('track manager hint');
  });
});

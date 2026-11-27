import { pollUntil } from '@digi/test-support';
import type { TestRuntime } from '../support.ts';

/** What the panel helpers need of a runtime: the session, the wait API's `ui` and its clock. */
export type Rt = Pick<TestRuntime, 'session' | 'ui' | 'clock'>;
/** The reason of a fixed wait after a panel input: the firmware redraws, and no page-change event exists. */
export const REDRAW = 'firmware redraw after a panel input (no page-change event)';
/** The reason of a fixed wait for the firmware to act on its own (a timer, a retry, a save): nothing observable yet. */
export const SETTLE = 'emulated time for the firmware to act (no observable state)';

/** The frame hash once the screen holds still: two reads 100 ms of emulated time apart are equal (a redraw may follow the first tap after a boot). */
export async function stableFrameHash(rt: Rt): Promise<string> {
  let last: string | undefined;
  const { hash } = await pollUntil(
    rt.clock,
    async () => {
      const hash = await rt.session.frameHash();
      const stable = hash === last;
      last = hash;
      return { hash, stable };
    },
    (r) => r.stable,
    { withinMs: 5000, intervalMs: 100, message: 'the frame holds still for 100 ms of emulated time' },
  );
  return hash;
}

/** Choose the original device list hint or a numeric override without asserting selected-track identity.
 * The BASIC chain uses selectSine for DT2 type readback; gain/deselect retains its audible-input contract. */
export async function pickSine(rt: Rt, selection?: number, ups = 0): Promise<void> {
  const hint = selection ?? (rt.session.options.device === 'dt2' ? 7 : 5);
  if (!Number.isInteger(hint) || hint < 0) throw new Error('FX input hint must be a nonnegative list position');
  await rt.ui.tap('SRC', { until: { drained: true }, held: 'FUNC' });
  await rt.ui.wait({ ms: 200, why: REDRAW });
  // `ups` UP presses first: every try starts from the top of the list
  for (let i = 0; i < ups; i++) {
    await rt.ui.tap('UP', { until: { drained: true } });
    await rt.ui.wait({ ms: 80, why: REDRAW });
  }
  for (let i = 0; i < hint; i++) {
    await rt.ui.tap('DOWN', { until: { drained: true } });
    await rt.ui.wait({ ms: 80, why: REDRAW });
  }
  await rt.ui.tap('YES', { until: { drained: true } });
  await rt.ui.wait({ ms: 300, why: REDRAW });
  await rt.ui.tap('YES', { until: { drained: true } });
  await rt.ui.wait({ ms: 300, why: REDRAW });
}

import { RuntimeEvents, type WaitRecord } from '@digi/dk-session';
import { emulatorClock, panelInput } from '@digi/test-support';
import { Ui } from './runtime/ui.ts';
import type { PatchedSession, TestRuntime } from './test-runtime.ts';
import { recordPhase } from './timing.ts';

/** A wait turned into a timing record. */
const recordWait = (r: WaitRecord): void => {
  recordPhase(`wait.${r.kind}`, r.wallMs, { what: r.what, clockMs: r.clockMs, ok: r.ok });
};

/** A patched session with the clock, `listen` and `ui` added (also for one built by hand). */
export function withWaitApi(ps: PatchedSession): TestRuntime {
  const { session, dk } = ps;
  const clock = emulatorClock(session);
  let current: RuntimeEvents | undefined;
  return {
    ...ps,
    clock,
    async listen(opts) {
      const events = new RuntimeEvents(dk, clock, { onWait: recordWait });
      await events.listen(opts);
      current = events;
      return events;
    },
    ui: new Ui(panelInput(session, clock), clock, () => current, recordWait, session),
  };
}

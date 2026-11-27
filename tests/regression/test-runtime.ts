import type { DkSession, Clock, RuntimeEvents } from '@digi/dk-session';
import type { Session } from '@digi/test-support';
import type { Ui } from './runtime/ui.ts';

export interface PatchedSession {
  readonly session: Session;
  readonly dk: DkSession;
  close(): Promise<void>;
}

export interface WaitApi {
  /** Emulated time for waits (`emulatorClock`: steps a paused daemon, waits on a running one). */
  readonly clock: Clock;
  /** Listens to the runtime log (see `RuntimeEvents.listen`) and makes it the events of `ui`. */
  listen(opts?: { fromBoot?: boolean }): Promise<RuntimeEvents>;
  /** Panel inputs and the waits after them. */
  readonly ui: Ui;
}

export type TestRuntime = PatchedSession & WaitApi;

export interface RuntimeCase extends TestRuntime {
  readonly epoch: number;
}

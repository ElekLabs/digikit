// Follows the runtime log of the emulator and of the device on the MIDI port chosen last, and puts what it finds in the workbench log.
// One poll loop per origin, once a second (log-follow-core.ts has the rules). Both read through the shared sessions of
// apps/web/src/transfer/target.ts, so their requests take their turn with the Transfer pane's.
import { createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js';
import { readFault, type DkSession } from '@digi/dk-session';
import type { LogInput, Origin } from './log-core.ts';
import { LogFollower, POLL_MS, type FollowSession, type OriginStatus } from './log-follow-core.ts';
import { logInputs } from './log-state.ts';

/** The protocol session as the follower uses it. */
export const followSession = (dk: DkSession): FollowSession => ({
  info: (quick) => dk.info(quick),
  logPage: (from, quick) => dk.logPage(from, quick),
  logConfig: (quick) => dk.logConfig(quick),
  modules: () => dk.modules(),
  fault: () => readFault(dk),
});

export interface FollowEnv {
  /** The Follow button. */
  follow(): boolean;
  /** A firmware flash is running: the Firmware page has the MIDI port. */
  flashing(): boolean;
  emulator: { running(): boolean; session(): Promise<FollowSession> };
  device: { port(): string | undefined; session(): Promise<FollowSession>; present(): Promise<boolean> };
  push?(inputs: LogInput[]): void;
  now?(): number;
  sleep?(ms: number): Promise<void>;
}

export interface LogFollow {
  status(): Record<Origin, OriginStatus>;
  /** The names of the modules each origin has loaded. */
  modules(): Record<Origin, string[]>;
}

/** Start following. Call it where an owner exists (a component): the loops end with it. */
export function createLogFollow(env: FollowEnv): LogFollow {
  const [status, setStatus] = createSignal<Record<Origin, OriginStatus>>({ emulator: 'off', device: 'off' });
  const [modules, setModules] = createSignal<Record<Origin, string[]>>({ emulator: [], device: [] });
  const now = env.now ?? Date.now;
  const sleep = env.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  const push = env.push ?? logInputs;
  const set = (origin: Origin, s: OriginStatus) =>
    setStatus((all) => (all[origin] === s ? all : { ...all, [origin]: s }));
  // a follower keeps where it was when Follow is turned off and on again: the log is not read twice
  const followers = new Map<string, LogFollower>();

  const follow = (
    origin: Origin,
    target: () => string | undefined,
    source: FollowEnv['emulator'] | FollowEnv['device'],
  ) => {
    // memos: the emulator's status changes with every frame, and a loop must restart only when its key or the Follow button changes
    const key = createMemo(target);
    const on = createMemo(env.follow);
    createEffect(() => {
      const k = key();
      if (!k || !on()) {
        set(origin, 'off');
        return;
      }
      let follower = followers.get(`${origin}:${k}`);
      if (follower) follower.resume();
      else {
        follower = new LogFollower({
          origin,
          session: source.session,
          present: 'present' in source ? source.present : undefined,
          now,
          out: {
            push,
            status: (s) => set(origin, s),
            modules: (names) => setModules((all) => ({ ...all, [origin]: names })),
          },
        });
        followers.set(`${origin}:${k}`, follower);
      }
      const f = follower;
      let stopped = false;
      onCleanup(() => {
        stopped = true;
      });
      // untracked: the first reads of the loop run inside the effect and must not restart it
      untrack(() => {
        void (async () => {
          while (!stopped) {
            let wait = POLL_MS;
            if (origin === 'device' && env.flashing()) f.pause();
            else {
              if (f.paused) f.resume();
              wait = await f.tick();
            }
            if (!stopped) await sleep(wait);
          }
        })();
      });
    });
  };

  follow('emulator', () => (env.emulator.running() ? 'emulator' : undefined), env.emulator);
  follow('device', env.device.port, env.device);
  return { status, modules };
}

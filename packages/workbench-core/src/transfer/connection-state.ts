// The one connection of the Transfer pane, kept for the life of the app (rules: connection-core.ts).
import { createSignal } from 'solid-js';
import { fmtVersion } from '@digi/dk-session';
import type { DkSession } from '@digi/dk-session';
import { isCancelled, isNoRuntime } from '../modules/backend';
import { CONNECTING, isLost, lostReason, step, type Connection, type ConnectionEvent } from './connection-core';

/** How long the target may stay disconnected before what was read from it is dropped. */
export const DISCONNECT_GRACE_MS = 3000;

/** `deviceName` is the app's (`TransferEnv.deviceName`): the name of the device with this id. */
export function createConnection(
  env: { deviceName(device: string): Promise<string> },
  invalidate?: () => void,
  /** `lost`: a request was lost (keep what was read, re-read it). `expired`: the target stayed disconnected beyond DISCONNECT_GRACE_MS. */
  hooks: { lost?: () => void; expired?: () => void } = {},
) {
  const [connection, setConnection] = createSignal<Connection>(CONNECTING);

  let selection = 0;
  let graceTimer: ReturnType<typeof setTimeout> | undefined;
  const noteConnection = (e: ConnectionEvent): void => {
    if (e.type === 'target') selection++;
    const next = step(connection(), e);
    // A lost request is not a new device: one unanswered request (a command the target does not know) must not empty the module
    // inventory and the pages built from it, and the next answer brings the indicator back. A read that fails clears the inventory itself.
    if (e.type === 'target' || e.type === 'closed' || e.type === 'noruntime') invalidate?.();
    if (e.type === 'lost') hooks.lost?.();
    setConnection(next);
    if (next.state === 'disconnected') {
      graceTimer ??= setTimeout(() => {
        graceTimer = undefined;
        if (connection().state === 'disconnected') hooks.expired?.();
      }, DISCONNECT_GRACE_MS);
    } else if (graceTimer !== undefined) {
      clearTimeout(graceTimer);
      graceTimer = undefined;
    }
  };

  /** Run a request of the session; a failure that means "the device cannot be reached" shows in the indicator, and is thrown on. */
  async function asked<T>(f: () => Promise<T>): Promise<T> {
    const asking = selection;
    noteConnection({ type: 'asking' });
    try {
      return await f();
    } catch (e) {
      if (asking !== selection) throw e;
      if (isCancelled(e)) noteConnection({ type: 'cancelled' });
      else if (isNoRuntime(e)) noteConnection({ type: 'noruntime', reason: (e as Error).message });
      else if (isLost(e)) noteConnection({ type: 'lost', reason: lostReason(e) });
      throw e;
    }
  }

  /** A reply on the target's session from a page that reads no INFO itself (the Memory panel, the Remote page): when the indicator does not
   * show the device, ask INFO once (short, no retries) so it does. A failure is left to the page's own error line. `still` tells whether the
   * target is the one asked: an answer of another target is dropped. */
  async function noteReply(s: DkSession, still: () => boolean): Promise<void> {
    if (connection().state === 'connected') return;
    try {
      const info = await s.info(true);
      const name = await env.deviceName(info.port);
      if (!still()) return;
      noteConnection({
        type: 'answered',
        device: { id: info.port, name, runtime: fmtVersion(info.runtime) },
      });
    } catch {
      /* the page says what failed */
    }
  }

  return { connection, setConnection, noteConnection, asked, noteReply };
}
export type ConnectionApp = ReturnType<typeof createConnection>;

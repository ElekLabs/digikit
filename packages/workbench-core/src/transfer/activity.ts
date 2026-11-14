// The glue of wait-core.ts's Activity to the app: the one tracker of the window, the transports that feed it, and the page's progress events.
import type { DkTransport } from '@digi/dk-session';
import { createActivity } from './wait-core.ts';

const tracker = createActivity();

/** Something happened that proves the device (or the job) is alive: a progress event, a job that starts. */
export const noteActivity = (): void => tracker.touch(Date.now());
/** Milliseconds of silence while a request waits for a reply; undefined when none waits. */
export const quietMs = (): number | undefined => tracker.quietFor(Date.now());

/**
 * Make `t` report to the tracker: a message sent or received is activity, and `recv` in flight is a request that waits. The transport is
 * changed in place (its other members stay as they are) and returned.
 */
export function instrument<T extends DkTransport>(t: T): T {
  const send = t.send.bind(t);
  const recv = t.recv.bind(t);
  t.send = async (msg) => {
    noteActivity();
    await send(msg);
  };
  t.recv = async (timeoutMs, signal) => {
    tracker.recvBegin();
    try {
      const got = await recv(timeoutMs, signal);
      if (got) noteActivity();
      return got;
    } finally {
      tracker.recvEnd();
    }
  };
  return t;
}

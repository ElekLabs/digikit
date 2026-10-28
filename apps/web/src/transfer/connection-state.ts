// The one connection of the Transfer pane, kept for the life of the window (rules: connection-core.ts).
import { createSignal } from 'solid-js';
import { isCancelled } from '../modules/backend';
import { CONNECTING, isLost, lostReason, step, type Connection, type ConnectionEvent } from './connection-core';

export const [connection, setConnection] = createSignal<Connection>(CONNECTING);

export const noteConnection = (e: ConnectionEvent): void => {
  setConnection(step(connection(), e));
};

/** Run a request of the session; a failure that means "the device cannot be reached" shows in the indicator, and is thrown on. */
export async function asked<T>(f: () => Promise<T>): Promise<T> {
  noteConnection({ type: 'asking' });
  try {
    return await f();
  } catch (e) {
    if (isCancelled(e)) noteConnection({ type: 'cancelled' });
    else if (isLost(e)) noteConnection({ type: 'lost', reason: lostReason(e) });
    throw e;
  }
}

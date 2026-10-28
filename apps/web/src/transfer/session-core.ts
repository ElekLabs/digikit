// One protocol session per target, shared by every page and panel that talks to it (pure; apps/web/tests/session-share.test.ts). Two
// sessions over the same link are not independent: the emulator's USB capture and a MIDI port give each reply to whichever session polls
// first, so each would take the other's replies and both would time out. One session per link keeps its requests in one queue, in turn.

/** The key of a target: the emulator, or one MIDI port. */
export const targetKey = (t: { kind: 'emulator' } | { kind: 'midi'; port: string } | { kind: 'none' }): string =>
  t.kind === 'midi' ? `midi:${t.port}` : t.kind;

export interface Cancellable {
  cancelPending(): void;
}

export interface Sessions<S extends Cancellable> {
  /** The session of the target, made at the first use and the same one for every caller after it. */
  get(key: string): S;
  /** Forget the session (a failed port): the next `get` makes a new one. Requests still waiting on the old one are not touched. */
  drop(key: string): void;
  /** Give up on the requests of the session that wait for the device; a target without a session has none. */
  cancel(key: string): void;
}

export function createSessions<S extends Cancellable>(make: (key: string) => S): Sessions<S> {
  const table = new Map<string, S>();
  return {
    get(key) {
      let s = table.get(key);
      if (!s) {
        s = make(key);
        table.set(key, s);
      }
      return s;
    },
    drop: (key) => void table.delete(key),
    cancel: (key) => table.get(key)?.cancelPending(),
  };
}

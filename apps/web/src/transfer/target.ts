// The target of the Transfer pane: the emulator (where the app has one), or a MIDI port with a real device on it. Every page that talks the
// digikit protocol (Transfer, Remote, the Memory panel of the workbench) uses the one selected here, through the one session of that target.
// The protocol client is the same in every case (`DkSession`); only the transport differs, and the app gives it (env.ts, `targets`).
import { createSignal } from 'solid-js';
import { DkSession } from '@digi/dk-session';
import type { DkTransport } from '@digi/dk-session';
import { instrument } from './activity';
import { noteConnection } from './connection-state';
import { transferEnv } from './env';
import { createSessions, targetKey } from './session-core';

/** `none`: the site before any MIDI port is there (it has no emulator); a request on it fails with a message that says what to do. */
export type Target = { kind: 'emulator' } | { kind: 'midi'; port: string } | { kind: 'none' };

const KEY = 'digikit.target';
const read = (): Target => {
  try {
    const t = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Target | null;
    if (t?.kind === 'midi' && typeof t.port === 'string' && t.port) return t;
  } catch {
    // no storage: the emulator
  }
  return { kind: 'emulator' };
};

export const [target, setTargetSignal] = createSignal<Target>(read());

export const NO_PORT = 'No MIDI port';
export const targetLabel = (t: Target): string =>
  t.kind === 'emulator' ? 'Emulator' : t.kind === 'none' ? NO_PORT : `MIDI: ${t.port}`;
export const sameTarget = (a: Target, b: Target): boolean =>
  a.kind === b.kind && (a.kind !== 'midi' || a.port === (b as { port: string }).port);

/** Start on this target without opening it or keeping the choice (the site picks its first target when the ports are known). */
export function startOn(t: Target): void {
  if (sameTarget(t, target())) return;
  opened = undefined;
  setTargetSignal(t);
  noteConnection({ type: 'target' });
}

/** The transport of each target in use (made once and instrumented once, so a new session of the same port reuses it). */
const transports = new Map<string, DkTransport>();

/** One session per target (session-core.ts): the emulator's, and the open MIDI port's. Every page and panel uses these, never a session of its own. */
const sessions = createSessions<DkSession>(
  (key) => new DkSession(transports.get(key) as DkTransport, { timeoutMs: 6000 }),
);
let opened: string | undefined;

async function openTransport(t: Target): Promise<void> {
  transports.set(targetKey(t), instrument(await transferEnv().targets.open(t)));
}

function forget(port: string): void {
  const key = targetKey({ kind: 'midi', port });
  sessions.drop(key);
  transports.delete(key);
}

/** The transport of a MIDI port ended by itself (the device was unplugged): the next request opens the port again, and fails if it is not there. */
export function portLost(port: string): void {
  if (opened === port) opened = undefined;
  forget(port);
}

/** Change the target: the next request uses the session of the new one (and opens the port). A MIDI port that cannot be opened leaves the old target. */
export async function selectTarget(next: Target): Promise<void> {
  if (sameTarget(next, target())) return;
  const { targets } = transferEnv();
  if (next.kind === 'midi') await openTransport(next);
  else if (opened) await targets.close?.({ kind: 'midi', port: opened });
  cancelPending();
  if (opened) forget(opened);
  opened = next.kind === 'midi' ? next.port : undefined;
  setTargetSignal(next);
  noteConnection({ type: 'target' });
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // the choice is not kept
  }
}

async function sessionOf(t: Target): Promise<DkSession> {
  const { targets } = transferEnv();
  if (t.kind === 'none')
    throw new Error(
      (await targets.whyNone?.()) ??
        'No MIDI port. Connect the device by USB, set its USB mode to MIDI, then pick it under Target.',
    );
  await targets.preflight?.(t);
  if (t.kind === 'midi') {
    if (opened !== t.port) {
      await openTransport(t);
      if (opened) forget(opened);
      opened = t.port;
    }
  } else if (!transports.has(targetKey(t))) {
    await openTransport(t);
  }
  return sessions.get(targetKey(t));
}

/** The one session of the selected target: its requests take turns on the one reply stream. A saved MIDI target is opened at its first use. */
export const session = (): Promise<DkSession> => sessionOf(target());

/**
 * The emulator's session whatever the selected target is (the Emulator page's Memory panel is about the emulator). It is THE emulator session,
 * the one `session()` returns while the emulator is the target, so every reader of the emulator's USB takes its turn in one queue.
 */
export const emulatorSession = (): Promise<DkSession> => sessionOf({ kind: 'emulator' });

/** Drop the session of a failed MIDI port (the next request opens it again). The emulator's session stays: it has no port to reopen. */
export function resetSession(): void {
  const t = target();
  if (t.kind === 'midi') sessions.drop(targetKey(t));
}

/** Give up on every request of the selected target's session that waits for the device (the Cancel button); each fails with the `cancelled` error. */
export function cancelPending(): void {
  sessions.cancel(targetKey(target()));
}

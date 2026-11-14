// The target of the Transfer pane: the emulator (where the app has one), or a MIDI port with a device on it. Every page that talks to the
// digikit protocol (Transfer, Remote, the Memory panel of the workbench) uses the one selected here, through the one session of that target.
// The protocol client is the same in every case (`DkSession`); only the transport differs, and the app gives it (env.ts, `targets`). The state of
// one app is a `TargetApp` (createTarget): the sessions come from the app's SessionHub, the choice is kept in its KeyValue.
import { createSignal } from 'solid-js';
import type { DkSession, DkTransport } from '@digi/dk-session';
import { NoRuntimeError } from '../modules/backend';
import type { KeyValue } from '../storage';
import { instrument } from './activity';
import type { ConnectionApp } from './connection-state';
import type { TransferEnv } from './env';
import { targetKey, type SessionHub } from './session-core';

/** `none`: the site before any MIDI port is there (it has no emulator); a request on it fails with a message that says what to do. */
export type Target = { kind: 'emulator' } | { kind: 'midi'; port: string } | { kind: 'none' };

const KEY = 'digikit.target';
const readTarget = (storage: KeyValue): Target => {
  try {
    const t = JSON.parse(storage.get(KEY) ?? 'null') as Target | null;
    if (t?.kind === 'midi' && typeof t.port === 'string' && t.port) return t;
  } catch {
    // no storage: the emulator
  }
  return { kind: 'emulator' };
};

/** The MIDI port chosen last, also while the Emulator is the target: the workbench's Log follows the device on it. Kept per profile. */
const MIDI_KEY = 'digikit.lastMidi';

export const NO_PORT = 'No MIDI port';
export const targetLabel = (t: Target): string =>
  t.kind === 'emulator' ? 'Emulator' : t.kind === 'none' ? NO_PORT : `MIDI: ${t.port}`;
export const sameTarget = (a: Target, b: Target): boolean =>
  a.kind === b.kind && (a.kind !== 'midi' || a.port === (b as { port: string }).port);

export const EMULATOR_NO_RUNTIME =
  'The emulator runs without the digikit runtime: tick Boot with runtime and reload it.';

export interface TargetDeps {
  env: Pick<TransferEnv, 'targets'>;
  storage: KeyValue;
  sessions: SessionHub;
  connection: Pick<ConnectionApp, 'noteConnection'>;
}

/** The target state of one app: the selected target, the sessions of the targets in use, and the transports under them. */
export function createTarget({ env, storage, sessions, connection }: TargetDeps) {
  const { noteConnection } = connection;
  const initial = readTarget(storage);
  const [target, setTargetSignal] = createSignal<Target>(initial);
  const [lastMidiPort, setLastMidiPort] = createSignal<string | undefined>(
    initial.kind === 'midi' ? initial.port : (storage.get(MIDI_KEY) ?? undefined),
  );

  /** Read the ports again in every selector shown (the desktop's title bar has the one selector; its pages' Refresh asks through this). */
  const [portScan, setPortScan] = createSignal(0);
  const rescanTargets = (): void => void setPortScan((n) => n + 1);

  /** The digikit runtime of the emulator as the app knows it from the last boot (the desktop sets it; the site has no emulator): 'unknown' while booting. */
  const [emulatorRuntime, setEmulatorRuntime] = createSignal<'unknown' | 'yes' | 'no'>('unknown');
  /** Throws NoRuntimeError for a target known to have no runtime (an emulator booted without it), before any request. */
  function checkRuntime(t: Target): void {
    if (t.kind === 'emulator' && emulatorRuntime() === 'no') throw new NoRuntimeError(EMULATOR_NO_RUNTIME);
  }

  /** The transport of each target in use (made once and instrumented once, so a new session of the same port reuses it). */
  const transports = new Map<string, DkTransport>();
  let opened: string | undefined;

  /** Start on this target without opening it or keeping the choice (the site picks its first target when the ports are known). */
  function startOn(t: Target): void {
    if (sameTarget(t, target())) return;
    opened = undefined;
    setTargetSignal(t);
    noteConnection({ type: 'target' });
  }

  async function openTransport(t: Target): Promise<void> {
    transports.set(targetKey(t), instrument(await env.targets.open(t)));
  }

  function forget(port: string): void {
    const key = targetKey({ kind: 'midi', port });
    sessions.drop(key);
    transports.delete(key);
  }

  /** The transport of a MIDI port ended by itself (the device was unplugged): the next request opens the port again, and fails if it is not there. */
  function portLost(port: string): void {
    if (opened === port) opened = undefined;
    forget(port);
  }

  /** Give up on every request of the selected target's session that waits for the device (the Cancel button); each fails with the `cancelled` error. */
  function cancelPending(): void {
    sessions.cancel(targetKey(target()));
  }

  /** Change the target: the next request uses the session of the new one (and opens the port). A MIDI port that cannot be opened leaves the old target. */
  async function selectTarget(next: Target): Promise<void> {
    if (sameTarget(next, target())) return;
    const { targets } = env;
    if (next.kind === 'midi') await openTransport(next);
    else if (opened) await targets.close?.({ kind: 'midi', port: opened });
    cancelPending();
    if (opened) forget(opened);
    opened = next.kind === 'midi' ? next.port : undefined;
    if (next.kind === 'midi') {
      setLastMidiPort(next.port);
      storage.set(MIDI_KEY, next.port);
    }
    setTargetSignal(next);
    noteConnection({ type: 'target' });
    storage.set(KEY, JSON.stringify(next));
  }

  async function sessionOf(t: Target): Promise<DkSession> {
    const { targets } = env;
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
    return sessions.get(targetKey(t), transports.get(targetKey(t)) as DkTransport);
  }

  /** The one session of the selected target: its requests take turns on the one reply stream. A saved MIDI target is opened at its first use. */
  const session = (): Promise<DkSession> => sessionOf(target());

  /**
   * The emulator's session whatever the selected target is (the Emulator page's Memory panel is about the emulator). It is THE emulator session,
   * the one `session()` returns while the emulator is the target, so every reader of the emulator's USB takes its turn in one queue.
   */
  const emulatorSession = (): Promise<DkSession> => sessionOf({ kind: 'emulator' });

  /**
   * The session of the MIDI port chosen last, whatever the selected target is (the Log follows the device on it). The port is opened if it is
   * not; its requests take their turn with the Transfer pane's when the port is also the target.
   */
  function lastMidiSession(): Promise<DkSession> {
    const port = lastMidiPort();
    if (!port) return Promise.reject(new Error(NO_PORT));
    return sessionOf({ kind: 'midi', port });
  }

  /** Drop the session of a failed MIDI port (the next request opens it again). The emulator's session stays: it has no port to reopen. */
  function resetSession(): void {
    const t = target();
    if (t.kind === 'midi') sessions.drop(targetKey(t));
  }

  /**
   * The transport of the selected MIDI port, opened if it is not (the Firmware page sends its messages through it, one by one, without the
   * session). The port is the target's: a request of the session in the meantime would mix its reply with the page's.
   */
  async function portTransport(): Promise<DkTransport> {
    const t = target();
    if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
    await sessionOf(t);
    return transports.get(targetKey(t)) as DkTransport;
  }

  /** Let go of the selected MIDI port (the device restarts): the next request opens it again, and fails while it is not there. */
  async function releasePort(): Promise<void> {
    const t = target();
    if (t.kind !== 'midi') return;
    await env.targets.close?.(t);
    portLost(t.port);
  }

  return {
    target,
    lastMidiPort,
    portScan,
    rescanTargets,
    emulatorRuntime,
    setEmulatorRuntime,
    checkRuntime,
    startOn,
    portLost,
    cancelPending,
    selectTarget,
    session,
    emulatorSession,
    lastMidiSession,
    resetSession,
    portTransport,
    releasePort,
  };
}
export type TargetApp = ReturnType<typeof createTarget>;

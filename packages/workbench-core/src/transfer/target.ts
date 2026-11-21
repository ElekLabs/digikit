// The target of the Transfer pane: the emulator (where the app has one), or a MIDI port with a device on it. Every page that talks to the
// digikit protocol (Transfer, Remote, the Memory panel of the workbench) uses the one selected here, through the one session of that target.
// The protocol client is the same in every case (`DkSession`); only the transport differs, and the app gives it (env.ts, `targets`). The state of
// one app is a `TargetApp` (createTarget): the sessions come from the app's SessionHub, the choice is kept in its KeyValue.
// This file selects the target and keeps one session per target. Each transport owns its link (open, recover, close); `DkSession` recovers the
// link between tries, so no page needs to know about it.
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
  connection: Pick<ConnectionApp, 'noteConnection' | 'noteReply' | 'connection'>;
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

  /**
   * The links in use, each made once and instrumented once. The emulator's link is made at the first use. There is at most one MIDI link (the
   * MidiHost has one slot): `{ port, transport }` of the port opened last. Each transport owns its open, recover and close.
   */
  let emulatorLink: Promise<DkTransport> | undefined;
  let midiLink: { port: string; transport: Promise<DkTransport> } | undefined;

  /** Start on this target without opening it or keeping the choice (the site picks its first target when the ports are known). */
  function startOn(t: Target): void {
    if (sameTarget(t, target())) return;
    setTargetSignal(t);
    noteConnection({ type: 'target' });
  }

  /** Let go of the MIDI link and forget its session (the next use opens the port again). Errors of the close are ignored. */
  function dropMidi(): Promise<void> {
    const old = midiLink;
    if (!old) return Promise.resolve();
    // forgotten at once (not after an await), so a caller that comes next opens a new link
    midiLink = undefined;
    const key = targetKey({ kind: 'midi', port: old.port });
    // requests still waiting on the dropped link end as cancelled
    sessions.cancel(key);
    sessions.drop(key);
    return old.transport
      .then((t) => t.close?.())
      .catch(() => {
        // the link is gone anyway
      });
  }

  /**
   * The link of the target, opened if it is not. Another MIDI port replaces the MIDI link: the old one is closed first, because the host's MIDI
   * slot is shared (closing it after the new port opened would close the new one).
   */
  function linkOf(t: Target): Promise<DkTransport> {
    if (t.kind === 'midi') {
      if (midiLink?.port === t.port) return midiLink.transport;
      // the entry is kept before anything is awaited, so a concurrent caller of the same port shares the open in progress
      const closing = dropMidi();
      const entry = { port: t.port, transport: closing.then(() => env.targets.open(t)).then(instrument) };
      midiLink = entry;
      entry.transport.catch(() => {
        if (midiLink === entry) midiLink = undefined;
      });
      return entry.transport;
    }
    if (!emulatorLink) {
      const opening = env.targets.open(t).then(instrument);
      emulatorLink = opening;
      opening.catch(() => {
        if (emulatorLink === opening) emulatorLink = undefined;
      });
    }
    return emulatorLink;
  }

  /** The link of the selected target if it is open (or opening). */
  const selectedLink = (): Promise<DkTransport> | undefined => {
    const t = target();
    return t.kind === 'midi' ? (midiLink?.port === t.port ? midiLink.transport : undefined) : undefined;
  };

  /** Give up on every request of the selected target's session that waits for the device (the Cancel button); each fails with the `cancelled` error. */
  function cancelPending(): void {
    sessions.cancel(targetKey(target()));
  }

  /** Change the target: the next request uses the session of the new one (and opens the port). A MIDI port that cannot be opened leaves the old target. */
  async function selectTarget(next: Target): Promise<void> {
    if (sameTarget(next, target())) return;
    // A dropped MIDI link cancels its own waiting requests (dropMidi). The emulator's link stays open, so its waiting requests are cancelled here.
    if (target().kind !== 'midi') cancelPending();
    if (next.kind === 'midi') await linkOf(next);
    else await dropMidi();
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
    const transport = await linkOf(t);
    let made: DkSession | undefined;
    // The session reports each request's outcome; only the selected target's reports reach the indicator.
    let noting = false;
    const onLink = (ok: boolean, reason?: string): void => {
      if (!sameTarget(t, target())) return;
      if (!ok) {
        noteConnection({ type: 'lost', reason: reason ?? 'No reply from the device.' });
        return;
      }
      const state = connection.connection().state;
      // noteReply asks INFO on the same session: its own reply must not start another one
      if (noting || !made || state === 'connected' || state === 'noruntime') return;
      noting = true;
      void connection.noteReply(made, () => sameTarget(t, target())).finally(() => (noting = false));
    };
    made = sessions.get(targetKey(t), transport, { onLink });
    return made;
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

  /** Make the selected target's link usable again (a device that re-enumerated). The emulator has no port to reopen: nothing happens. */
  async function reconnect(): Promise<void> {
    const l = selectedLink();
    if (l) await (await l).recover?.();
  }

  /**
   * The transport of the selected MIDI port, opened if it is not (the Firmware page sends its messages through it, one by one, without the
   * session). The port is the target's: a request of the session in the meantime would mix its reply with the page's.
   */
  async function portTransport(): Promise<DkTransport> {
    const t = target();
    if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
    await sessionOf(t);
    return (midiLink as { transport: Promise<DkTransport> }).transport;
  }

  /** Let go of the selected MIDI port (the device restarts): the next request opens it again, and fails while it is not there. */
  async function releasePort(): Promise<void> {
    const t = target();
    if (t.kind !== 'midi' || midiLink?.port !== t.port) return;
    await dropMidi();
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
    cancelPending,
    selectTarget,
    session,
    emulatorSession,
    lastMidiSession,
    reconnect,
    portTransport,
    releasePort,
  };
}
export type TargetApp = ReturnType<typeof createTarget>;

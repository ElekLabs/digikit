// A TransferApp over fakes, for tests (the way `createFakeHost` serves the Host): an in-memory KeyValue, a SessionHub whose sessions are objects
// the test gives, and a TransferEnv of stubs that a test overrides in part. `createTransferApp` is the real one; only what it is given is fake.
import type { DkSession, DkTransport } from '@digi/dk-session';
import type { NeedsLike } from '../modules/dkneeds';
import { memoryKeyValue, type KeyValue } from '../storage';
import { createTransferApp, type TransferApp } from './app';
import type { TransferEnv } from './env';
import type { SessionHub } from './session-core';

/** A transport that never answers and ignores what is sent. */
export const silentTransport = (): DkTransport => ({ send: () => {}, recv: async () => null });

export interface FakeSessions extends SessionHub {
  /** The keys of the sessions made, in order. */
  made: string[];
  /** The keys dropped and cancelled, in order. */
  dropped: string[];
  cancelled: string[];
}

/** A hub whose session for a key is `make(key)` (made once per key until it is dropped). */
export function createFakeSessions(make: (key: string, transport: DkTransport) => unknown = () => ({})): FakeSessions {
  const table = new Map<string, DkSession>();
  const hub: FakeSessions = {
    made: [],
    dropped: [],
    cancelled: [],
    get(key, transport) {
      let s = table.get(key);
      if (!s) {
        s = make(key, transport) as DkSession;
        table.set(key, s);
        hub.made.push(key);
      }
      return s;
    },
    drop(key) {
      table.delete(key);
      hub.dropped.push(key);
    },
    cancel(key) {
      hub.cancelled.push(key);
    },
  };
  return hub;
}

export interface FakeTransferApp extends TransferApp {
  storage: ReturnType<typeof memoryKeyValue>;
  sessions: FakeSessions;
}

/** A needs reader for tests that do not read real .dkm files: no file has needs, and every request fits (an empty answer). */
const stubNeeds: NeedsLike = {
  read: () => ({ error: 'stub needs reader' }),
  of: () => null,
  fit: () => ({ each: [], set: null, line: '' }),
};

/** `env` replaces parts of the stub environment; `storage` and `sessions` replace the fakes (the app's own are returned). */
export function createFakeTransferApp(
  opts: {
    env?: Partial<TransferEnv>;
    storage?: KeyValue;
    sessions?: FakeSessions;
    /** The needs reader (default: a stub that finds no needs and no fit). */
    needs?: () => Promise<NeedsLike>;
  } = {},
): FakeTransferApp {
  const storage = (opts.storage ?? memoryKeyValue()) as ReturnType<typeof memoryKeyValue>;
  const sessions = opts.sessions ?? createFakeSessions();
  const env: TransferEnv = {
    library: { scan: async () => [], read: async () => new Uint8Array() },
    wasm: { bytes: async () => new ArrayBuffer(0) },
    deviceName: async (device) => device,
    targets: { emulator: true, ports: async () => [], open: async () => silentTransport() },
    logRun: () => {},
    ...opts.env,
  };
  return {
    ...createTransferApp(env, { storage, sessions, needs: opts.needs ?? (async () => stubNeeds) }),
    storage,
    sessions,
  };
}

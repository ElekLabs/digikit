import type { DkInfoFull, DkSession } from './dkupload-core.ts';
import type { RuntimeEvents } from './runtime-events.ts';
import { poll } from './waits.ts';

/** Raw firmware identities. Class identifies the C++ view type, not the content of a reused modal. */
export interface FirmwareView {
  object: number;
  classId: number;
}

export interface ViewObserverState {
  supported: boolean;
  enabled: boolean;
  generation: number;
  /** Undefined until the first good UI pass. Zero object and class mean an empty stack. */
  snapshot?: FirmwareView;
}

export function parseViewObserverState(bytes: Uint8Array): ViewObserverState {
  if (bytes.length !== 16 || bytes[0] !== 1 || ![bytes[1], bytes[2], bytes[3]].every((b) => b === 0 || b === 1))
    throw new Error('view observer: malformed or unsupported LOGCFG reply');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const generation = view.getUint32(4);
  const supported = bytes[1] === 1;
  const enabled = bytes[2] === 1;
  if (enabled !== (generation !== 0) || (enabled && !supported) || (bytes[3] === 1 && !enabled))
    throw new Error('view observer: inconsistent LOGCFG reply');
  return {
    supported,
    enabled,
    generation,
    snapshot: bytes[3] === 1 ? { object: view.getUint32(8), classId: view.getUint32(12) } : undefined,
  };
}

export class ViewObserverUnsupported extends Error {
  constructor(target: string, why: string) {
    super(`${target} does not support ui.view observation (${why}). No panel inputs were sent.`);
    this.name = 'ViewObserverUnsupported';
  }
}

type ViewSession = Pick<DkSession, 'clock' | 'logConfig' | 'setLogConfig' | 'viewObserver' | 'setViewObserver'> & {
  info(): Promise<Pick<DkInfoFull, 'port' | 'log_sink'>>;
};

/**
 * Check support before running the scenario, then arm the observer and wait
 * for its initial snapshot. Reuse the caller's normal RuntimeEvents cursor for
 * VIEW_CHANGED. No event is emitted just to acknowledge a baseline.
 */
export async function withViewObserver<T>(
  session: ViewSession,
  events: Pick<RuntimeEvents, 'listen'>,
  run: (snapshot: FirmwareView, generation: number) => Promise<T>,
  { target, withinMs = 2000 }: { target?: string; withinMs?: number } = {},
): Promise<T> {
  const info = await session.info();
  const label = target ?? info.port;
  if (!info.log_sink) throw new ViewObserverUnsupported(label, 'digikit/debug is not loaded');
  const state = await session.viewObserver();
  if (!state?.supported)
    throw new ViewObserverUnsupported(
      label,
      state ? 'no view stack is mapped for this firmware' : 'update digikit/debug',
    );
  if (state.enabled) throw new Error(`${label} already has an active view observer`);

  const log = await session.logConfig();
  let generation = 0;
  try {
    await events.listen();
    const armed = await session.setViewObserver(true);
    generation = armed.generation;
    const ready = await poll(
      session.clock,
      async () => {
        const current = await session.viewObserver();
        if (!current?.enabled || current.generation !== generation)
          throw new Error(`${label} view observation stopped or was replaced`);
        return current.snapshot;
      },
      (value): value is FirmwareView => value !== undefined,
      { withinMs, message: `${label} did not publish an initial UI view snapshot` },
    );
    if (!ready) throw new Error(`${label} did not publish an initial UI view snapshot`);
    return await run(ready, generation);
  } finally {
    try {
      // Don't stop an observer belonging to a different capture.
      if (generation && (await session.viewObserver())?.generation === generation) await session.setViewObserver(false);
    } finally {
      await session.setLogConfig(log.level, log.modules);
    }
  }
}

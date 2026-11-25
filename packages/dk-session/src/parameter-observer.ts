import type { DkInfoFull, DkSession } from './dkupload-core.ts';
import type { RuntimeEvents } from './runtime-events.ts';
import type { RuntimeEvent } from './runtime-log.ts';
import { poll } from './waits.ts';

export type ParameterScope = { scope: 'track'; track: number } | { scope: 'kit' };
export type ParameterKey = ParameterScope & { kind: string; word: number };

type ParameterWatchProfile = {
  name: string;
  parameterWatches?: Readonly<
    Record<string, Readonly<Record<string, { scope: ParameterScope['scope']; word: number }>>>
  >;
};

export class ParameterObserverUnsupported extends Error {
  constructor(target: string, kind: string, reason: string) {
    super(`${target} cannot observe ${kind} (${reason}). No panel inputs were sent.`);
    this.name = 'ParameterObserverUnsupported';
  }
}

/** Resolve a semantic kind on the verified firmware version, before any panel input. */
export function resolveParameterWatch(
  profile: ParameterWatchProfile,
  version: string,
  kind: string,
  scope: ParameterScope,
): ParameterKey {
  const table = profile.parameterWatches?.[version];
  const mapping = table && Object.hasOwn(table, kind) ? table[kind] : undefined;
  if (!mapping || mapping.scope !== scope.scope)
    throw new ParameterObserverUnsupported(`${profile.name} OS ${version}`, kind, `no ${scope.scope} mapping`);
  const key = { ...scope, kind, word: mapping.word };
  parameterAddress(key);
  return key;
}

export function parameterAddress(key: ParameterKey): readonly [number, number, number] {
  const track = key.scope === 'track' ? key.track - 1 : 0;
  if (!Number.isInteger(track) || track < 0 || track >= 16)
    throw new RangeError('Parameter observation uses track numbers 1 to 16');
  if (!Number.isInteger(key.word) || key.word < 0 || key.word > 255)
    throw new RangeError('Parameter observation uses word indices 0 to 255');
  return [key.scope === 'track' ? 1 : 2, track, key.word];
}

export interface ParameterObserverState {
  supported: boolean;
  generation: number;
  /** Signed raw value from the firmware. Undefined before the first good read. */
  value?: number;
}

export function parseParameterObserver(bytes: Uint8Array): ParameterObserverState {
  if (bytes.length !== 16 || bytes[0] !== 1 || (bytes[1]! & ~7) !== 0)
    throw new Error('parameter observer: malformed or unsupported LOGCFG reply');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const flags = bytes[1]!;
  const generation = view.getUint32(4);
  if (Boolean(flags & 2) !== (generation !== 0) || (flags & 4 && (flags & 3) !== 3))
    throw new Error('parameter observer: inconsistent LOGCFG reply');
  return { supported: Boolean(flags & 1), generation, value: flags & 4 ? view.getInt32(8) : undefined };
}

type ParameterWatchSession = Pick<
  DkSession,
  | 'clock'
  | 'logConfig'
  | 'setLogConfig'
  | 'probeParameter'
  | 'startParameterObserver'
  | 'parameterObserver'
  | 'stopParameterObserver'
> & { info(): Promise<Pick<DkInfoFull, 'port' | 'log_sink'>> };

/** Keep scope and generation checks out of tests and capture scripts. */
export function matchesParameterChange(
  event: RuntimeEvent,
  key: ParameterKey,
  generation: number,
  match: (value: number, old: number) => boolean,
): boolean {
  const [scope, track, word] = parameterAddress(key);
  return (
    event.kind === 'VALUE_CHANGED' &&
    event.args[0] === scope &&
    event.args[1] === ((track << 8) | word) &&
    event.args[4] === generation &&
    match(event.args[3]! | 0, event.args[2]! | 0)
  );
}

/** Uses the caller's existing log cursor. The snapshot itself is not a log event. */
export async function withParameterObserver<T>(
  session: ParameterWatchSession,
  events: Pick<RuntimeEvents, 'listen'>,
  key: ParameterKey,
  run: (initial: number, generation: number) => Promise<T>,
  { target, withinMs = 2000 }: { target?: string; withinMs?: number } = {},
): Promise<T> {
  const info = await session.info();
  const label = target ?? info.port;
  if (!info.log_sink) throw new ParameterObserverUnsupported(label, key.kind, 'digikit/debug is not loaded');
  const support = await session.probeParameter(key);
  if (!support?.supported)
    throw new ParameterObserverUnsupported(
      label,
      key.kind,
      support ? 'readback is not available on this image' : 'update digikit/debug',
    );
  if (support.generation) throw new Error(`${label} already has an active parameter observer`);

  const log = await session.logConfig();
  let generation = 0;
  try {
    await events.listen();
    generation = (await session.startParameterObserver(key)).generation;
    const initial = await poll(
      session.clock,
      async () => {
        const state = await session.parameterObserver();
        if (!generation || state.generation !== generation)
          throw new Error(`${label} parameter observation stopped or was replaced`);
        return state.value;
      },
      (value) => value !== undefined,
      { withinMs, message: `${label} did not publish a baseline for ${key.kind}` },
    );
    if (initial === undefined) throw new Error(`${label} did not publish a baseline for ${key.kind}`);
    return await run(initial, generation);
  } finally {
    try {
      if (generation) await session.stopParameterObserver(generation);
    } finally {
      await session.setLogConfig(log.level, log.modules);
    }
  }
}

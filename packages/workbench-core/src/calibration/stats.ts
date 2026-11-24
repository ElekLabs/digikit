// The counters a calibration capture reads: the DSP stats window (P8d `PROFILE_DSP`) and the runtime spans (`PROFILE_STATE`, `PROFILE_READ`).
// `StatsSource` is what the runner needs; `sessionStatsSource` is written against the dk-session API (`profileAll`, and `profileDsp` of P8d,
// feature-checked: a session without it gives spans only). Tests give a fake.
import { spansPart, type BuildInput, type SpansPart } from '../profile/profile-artifact.ts';

import { DkError, type DkSession, type DspStats } from '@digi/dk-session';

import type { TraceMarker } from '../profile/trace.ts';

export type { DspRow, DspStats } from '@digi/dk-session';

export interface StatsReading {
  dsp?: DspStats;
  spans?: SpansPart;
  trace?: BuildInput['trace'];
  traceError?: string;
}

export interface StatsSource {
  /** What the target can count. */
  capabilities(): Promise<{ dsp: boolean; spans: boolean }>;
  /** Count, from zero: the spans are reset then started, the DSP counters started. */
  start(): Promise<void>;
  /** Forget what was counted (spans reset, DSP stats cleared: a new generation) and go on counting. `generation`: the clear generation of the DSP stats (absent without them). */
  clear(): Promise<{ generation?: number }>;
  mark?(name: string): Promise<void>;
  stop(): Promise<void>;
  /** The counters now. The DSP part is the newest complete sweep of the current generation, absent when there is none yet. */
  read(): Promise<StatsReading>;
}

/** The members of `DkSession` the real source uses. */
export interface StatsSession {
  profileState(
    op: Parameters<DkSession['profileState']>[0],
  ): Promise<Partial<Awaited<ReturnType<DkSession['profileState']>>> | void>;
  profileTrace?: DkSession['profileTrace'];
  profileAll(): Promise<{
    hz: number;
    elapsedTicks: bigint;
    spans: { kind: 'module' | 'phase' | 'poll'; index: number; calls: number; sumTicks: bigint; maxTicks: number }[];
  }>;
  profileDsp(op?: 'get' | 'start' | 'stop' | 'clear'): Promise<DspStats>;
}

/** The counters of a session. `nameOf` names a span (modules by their loaded name). A debug module without PROFILE_DSP (an older one) gives spans only. */
export function sessionStatsSource(
  session: StatsSession,
  nameOf: (s: { kind: 'module' | 'phase' | 'poll'; index: number }) => string,
  trace = false,
): StatsSource {
  let generation: number | undefined;
  let markers: TraceMarker[] = [];
  let unavailable: string | undefined;
  let dsp: boolean | undefined;
  // only the runtime's answer to an unknown command (E_PROTO: this debug module has no PROFILE_DSP) means "no DSP stats"; a timeout, busy or not-ready target throws, so a
  // calibration never silently becomes spans-only
  const hasDsp = async (): Promise<boolean> => {
    if (dsp !== undefined) return dsp;
    try {
      await session.profileDsp('get');
      dsp = true;
    } catch (e) {
      if (!(e instanceof DkError) || e.status !== 'E_PROTO') throw e;
      dsp = false;
    }
    return dsp;
  };
  return {
    async capabilities() {
      return { dsp: await hasDsp(), spans: true };
    },
    async start() {
      generation = undefined;
      markers = [];
      unavailable = undefined;
      // counters an earlier run left on (a stale generation) are stopped first
      await session.profileState('stop');
      if (await hasDsp()) await session.profileDsp('stop');
      await session.profileState('reset');
      await session.profileState('start');
      if (await hasDsp()) {
        await session.profileDsp('clear');
        await session.profileDsp('start');
      }
    },
    async clear() {
      if (trace) {
        if (session.profileTrace && (await session.profileState('get'))?.traceVersion === 1)
          await session.profileState('startTrace');
        else unavailable = 'The debug module does not support ColdFire event tracing.';
      }
      // PROFILE_STATE reset resets the trace origin to the same zero as the elapsed span clock.
      await session.profileState('reset');
      if (trace && !unavailable) {
        generation = (await session.profileState('startTrace'))?.traceGeneration;
        if (generation === undefined) throw new Error('The debug module did not return a trace generation.');
        markers = [{ name: 'Capture start', atTicks: '0' }];
      }
      if (!(await hasDsp())) return {};
      return { generation: (await session.profileDsp('clear')).generation };
    },
    async mark(name) {
      if (generation === undefined) return;
      const elapsed = (await session.profileState('get'))?.elapsedTicks;
      if (elapsed !== undefined) markers.push({ name, atTicks: String(elapsed) });
    },
    async stop() {
      const result = await session.profileState('stop');
      if (generation !== undefined && result?.elapsedTicks !== undefined)
        markers.push({ name: 'Capture stop', atTicks: String(result.elapsedTicks) });
      if (await hasDsp()) await session.profileDsp('stop');
    },
    async read() {
      const spans = spansPart(await session.profileAll(), nameOf);
      const reading = (await hasDsp()) ? await session.profileDsp('get') : undefined;
      const result: StatsReading = { spans, ...(reading ? { dsp: reading } : {}) };
      if (unavailable) result.traceError = unavailable;
      if (generation !== undefined && session.profileTrace) {
        try {
          result.trace = { ...(await session.profileTrace(generation)), markers };
        } catch (error) {
          result.traceError = error instanceof Error ? error.message : String(error);
        }
      }
      return result;
    },
  };
}

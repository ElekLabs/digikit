// The counters a calibration capture reads: the DSP stats window (P8d `PROFILE_DSP`) and the runtime spans (`PROFILE_STATE`, `PROFILE_READ`).
// `StatsSource` is what the runner needs; `sessionStatsSource` is written against the dk-session API (`profileAll`, and `profileDsp` of P8d,
// feature-checked: a session without it gives spans only). Tests give a fake.
import { spansPart, type SpansPart } from '../profile/profile-artifact.ts';

/** One row of the DSP stats window (dk-session `DspRow`, P8d). */
export interface DspRow {
  row: number;
  kind: 'machine' | 'effect';
  calls: number;
  sumCycles: bigint;
  maxCall: number;
  maxBlock: number;
}

/** The state and newest complete sweep of the DSP stats window (dk-session `DspStats`, P8d). */
export interface DspStats {
  on: boolean;
  generation: number;
  counts: { sweeps: number; gaps: number; ignored: number };
  sweep?: { number: number; blocks: number; maxStockBlockDelta: number; overruns: number; rows: DspRow[] };
}

export interface StatsReading {
  dsp?: DspStats;
  spans?: SpansPart;
}

export interface StatsSource {
  /** What the target can count. */
  capabilities(): Promise<{ dsp: boolean; spans: boolean }>;
  /** Count, from zero: the spans are reset then started, the DSP counters started. */
  start(): Promise<void>;
  /** Forget what was counted (spans reset, DSP stats cleared: a new generation) and go on counting. */
  clear(): Promise<void>;
  stop(): Promise<void>;
  /** The counters now. The DSP part is the newest complete sweep of the current generation, absent when there is none yet. */
  read(): Promise<StatsReading>;
}

/** The members of `DkSession` the real source uses. `profileDsp` is absent on a dk-session without P8d. */
export interface StatsSession {
  profileState(op: 'get' | 'start' | 'stop' | 'reset'): Promise<unknown>;
  profileAll(): Promise<{
    hz: number;
    elapsedTicks: bigint;
    spans: { kind: 'module' | 'phase' | 'poll'; index: number; calls: number; sumTicks: bigint; maxTicks: number }[];
  }>;
  profileDsp?(op?: 'get' | 'start' | 'stop' | 'clear'): Promise<DspStats>;
}

/** The counters of a session. `nameOf` names a span (modules by their loaded name). */
export function sessionStatsSource(
  session: StatsSession,
  nameOf: (s: { kind: 'module' | 'phase' | 'poll'; index: number }) => string,
): StatsSource {
  return {
    async capabilities() {
      return { dsp: typeof session.profileDsp === 'function', spans: true };
    },
    async start() {
      await session.profileState('reset');
      await session.profileState('start');
      await session.profileDsp?.('clear');
      await session.profileDsp?.('start');
    },
    async clear() {
      await session.profileState('reset');
      await session.profileDsp?.('clear');
    },
    async stop() {
      await session.profileState('stop');
      await session.profileDsp?.('stop');
    },
    async read() {
      const spans = spansPart(await session.profileAll(), nameOf);
      const dsp = session.profileDsp ? await session.profileDsp('get') : undefined;
      return { spans, ...(dsp ? { dsp } : {}) };
    },
  };
}

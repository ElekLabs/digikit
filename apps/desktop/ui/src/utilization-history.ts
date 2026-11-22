import { createEffect, createMemo, createSignal, on } from 'solid-js';
import type { NativeAudioStatus } from '@digi/workbench-core/runtime/runtime.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import type { ReceivedHealth } from './target-health';

export const UTILIZATION_WINDOW_MS = 60_000;

/** Remote HEALTH is emitted every 10 s and observed on a 5 s poll; allow for an exchange in flight. */
export function utilizationIsFresh(elapsed: number, estimated: boolean): boolean {
  return elapsed <= (estimated ? 3000 : 20_000);
}

export interface UtilizationPoint {
  at: number;
  cf: number | null;
  dsp: number | null;
}

const valid = (value?: number | null) => (value != null && Number.isFinite(value) && value >= 0 ? value : null);

/** One minute of readings from the current binding. A missing reading breaks each trace. */
export function createUtilizationHistory(
  binding: () => TargetBinding,
  deviceHealth: () => ReceivedHealth | undefined,
  clock = Date.now,
) {
  const history = createMemo(() => {
    const source = binding();
    const [points, setPoints] = createSignal<UtilizationPoint[]>([]);
    const append = (at: number, cf?: number | null, dsp?: number | null) => {
      const next = { at, cf: valid(cf), dsp: valid(dsp) };
      setPoints((previous) => {
        const last = previous.at(-1);
        if (next.cf === null && next.dsp === null && last?.cf == null && last?.dsp == null) return previous;
        return [...previous.filter((p) => p.at >= at - UTILIZATION_WINDOW_MS && p.at < at), next];
      });
    };
    if (source.target.kind === 'midi') {
      createEffect(
        on(deviceHealth, (health) => {
          if (health && health.source !== source) return;
          append(health?.received ?? clock(), health?.cpu, health?.dsp);
        }),
      );
    }
    return {
      points,
      emulator(load: NativeAudioStatus['load']) {
        if (source.target.kind === 'emulator') append(clock(), load?.cf.real_pct, load?.dsp?.real_pct);
      },
    };
  });
  return history;
}

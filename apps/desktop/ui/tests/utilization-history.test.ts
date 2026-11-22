import { createRoot, createSignal } from 'solid-js';
import { expect, test } from 'vitest';
import type { TargetBinding, Target } from '@digi/workbench-core/transfer/target.ts';
import type { ReceivedHealth } from '../src/target-health';
import { createUtilizationHistory, utilizationAt, utilizationIsFresh } from '../src/utilization-history';

function binding(target: Target): TargetBinding {
  return {
    target,
    check() {},
    beforeLeave: () => () => true,
    session: async () => {
      throw new Error('Unexpected device poll');
    },
  };
}

test('history takes measured readings only from the active port and resets on a target switch', () => {
  const a = binding({ kind: 'midi', port: 'A' });
  const b = binding({ kind: 'midi', port: 'B' });
  const [source, select] = createSignal(a);
  const [sample, receive] = createSignal<ReceivedHealth>();
  const { history, dispose } = createRoot((dispose) => ({
    history: createUtilizationHistory(source, sample, () => 1002),
    dispose,
  }));
  try {
    const health = { source: a, received: 1000, cpu: 25, dsp: 30, peak: 45, seq: 5, blocks: 100 };
    receive(health);
    expect(history().points()).toEqual([{ at: 1000, cf: 25, dsp: 30 }]);
    select(b);
    expect(history().points()).toEqual([]);
    receive({ ...health, received: 1001 });
    expect(history().points()).toEqual([]);
    receive({ ...health, source: b, cpu: 0, dsp: null });
    receive(undefined);
    expect(history().points()).toEqual([
      { at: 1000, cf: 0, dsp: null },
      { at: 1002, cf: null, dsp: null },
    ]);
  } finally {
    dispose();
  }
});

test('emulator history preserves overload, prunes old readings and ignores late replies from the last binding', () => {
  let at = 1000;
  const [source, select] = createSignal(binding({ kind: 'emulator' }));
  const { history, dispose } = createRoot((dispose) => ({
    history: createUtilizationHistory(
      source,
      () => undefined,
      () => at,
    ),
    dispose,
  }));
  const load = { window_s: 1, cf: { real_pct: 124, model_pct: 100, real_high_pct: 130 }, dsp: null };
  try {
    const old = history();
    old.emulator(load);
    at += 61_000;
    old.emulator(load);
    expect(old.points()).toEqual([{ at, cf: 124, dsp: null }]);
    select(binding({ kind: 'midi', port: 'B' }));
    old.emulator(load);
    expect(history().points()).toEqual([]);
  } finally {
    dispose();
  }
});

test('hardware HEALTH intervals tolerate poll latency and emulator samples expire at their own cadence', () => {
  expect(utilizationIsFresh(15_200, false)).toBe(true);
  expect(utilizationIsFresh(25_000, false)).toBe(false);
  expect(utilizationIsFresh(1800, true)).toBe(true);
  expect(utilizationIsFresh(5100, true)).toBe(false);
});

test('cursor follows each drawn segment, preserving endpoints and gaps independently per processor', () => {
  const points = [
    { at: 1000, cf: 0, dsp: null },
    { at: 11000, cf: 80, dsp: 65 },
  ];
  expect(utilizationAt(points, 3500, false)).toEqual({ at: 3500, cf: 20, dsp: null, interpolated: true });
  expect(utilizationAt(points, 11000, false)).toEqual({ ...points[1], interpolated: false });
  expect(utilizationAt(points, 3500, true)).toBeUndefined();
  expect(utilizationAt(points, 500, false)).toBeUndefined();
  expect(utilizationAt(points, 11500, false)).toBeUndefined();
});

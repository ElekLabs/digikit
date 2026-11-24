import { describe, expect, test } from 'vitest';
import { readPerformanceCaptures } from '../src/profile/performance-capture.ts';

const effect = {
  schema: 'dk.profile/1',
  run: {
    id: 'effect-A',
    case_id: 'effect',
    dispatch: [
      { row: 8, kind: 'machine', module: 'test/wrong-map' },
      { row: 8, kind: 'effect', module: 'test/gain', route: 'send' },
    ],
  },
  dsp: { unit: 'insn', blocks: 50, overruns: 1 },
  dsp_rows: [
    { row: 8, kind: 'effect', calls: 49, sum_cycles: 15000, max_call: 340, max_block: 340 },
    { row: 9, kind: 'effect', calls: 50, sum_cycles: 5000, max_call: 120, max_block: 120 },
  ],
};

describe('saved performance captures', () => {
  test('keeps span totals and PCs in their original runs even when no runtime is present', () => {
    const [send, rig] = readPerformanceCaptures(
      {
        schema: 'dk.calcaptures/1',
        captures: [
          {
            ...effect,
            span_clock_hz: 1_000_000,
            span_elapsed_ticks: 1000,
            spans: [
              { owner: 0x101, kind: 'phase', name: 'idle', calls: 0, sum_ticks: 0, max_ticks: 0 },
              { owner: 0x100, kind: 'phase', name: 'early', calls: 5, sum_ticks: 250, max_ticks: 99 },
            ],
          },
          {
            schema: 'dk.profile/1',
            run: { id: 'rig', case_id: 'effect' },
            raw: [{ cpu: 'sharc', series: 'pc', addr: 0x120, count: 150 }],
          },
        ],
      },
      'run.json',
    );
    expect(send.spans).toMatchObject({
      clockHz: 1_000_000,
      elapsedTicks: 1000,
      rows: [
        { owner: 0x100, name: 'early', calls: 5, totalUs: 250, maxUs: 99, share: 0.25 },
        { name: 'idle', calls: 0, totalUs: 0, maxUs: 0, share: 0 },
      ],
    });
    expect(send.hasHotspots).toBe(false);
    expect(rig.spans).toBeUndefined();
    expect(rig.hasHotspots).toBe(true);
  });

  test('opens span-only captures and preserves unknown timing and share as missing', () => {
    const [capture] = readPerformanceCaptures(
      {
        schema: 'dk.profile/1',
        spans: [{ name: 'early', calls: 5, sum_ticks: 250, max_ticks: 99 }],
      },
      'spans.json',
    );
    expect(capture.spans?.rows[0]).toMatchObject({
      name: 'early',
      calls: 5,
      totalTicks: 250,
      maxTicks: 99,
      totalUs: undefined,
      share: undefined,
    });
  });
  test('reads the recorded unit and identifies the dispatch by kind and row', () => {
    const [capture] = readPerformanceCaptures(effect, 'capture.json');
    expect(capture.label).toBe('effect-A');
    expect(capture.trace).toBeUndefined();
    expect(capture.dsp).toMatchObject({
      unit: 'instructions',
      blocks: 50,
      overruns: 1,
      rows: [
        { row: 8, module: 'test/gain', route: 'send', calls: 49, meanBlock: 300, maxBlock: 340, maxCall: 340 },
        { row: 9, calls: 50, meanBlock: 100, maxBlock: 120, maxCall: 120 },
      ],
    });
    expect(capture.dsp?.rows[1]?.module).toBeUndefined();
  });

  test('keeps both DSP-only and trace-only cases in a mixed run', () => {
    const [costs, trace] = readPerformanceCaptures(
      {
        schema: 'dk.calcaptures/1',
        side: 'dev',
        captures: [
          { ...effect, dsp: { ...effect.dsp, unit: 'cycles' } },
          { schema: 'dk.profile/1', run: { case_id: 'trace' }, trace: { hz: 132000000, dropped: 0, events: [] } },
        ],
      },
      'run.json',
    );
    expect(costs.dsp?.unit).toBe('cycles');
    expect(trace.label).toBe('run.json · 2');
    expect(trace.trace?.trace.hz).toBe(132000000);
    expect(trace.dsp).toBeUndefined();
  });

  test('accepts a hotspot-only profile and keeps the original document', () => {
    const profile = {
      schema: 'dk.profile/1',
      raw: [{ cpu: 'sharc', series: 'pc', addr: 0x120, count: 150 }],
    };
    const [capture] = readPerformanceCaptures(profile, 'rig.json');
    expect(capture.hasHotspots).toBe(true);
    expect(capture.document).toBe(profile);
    expect(capture.dsp).toBeUndefined();
    expect(capture.trace).toBeUndefined();
  });

  test('refuses counters with invalid units or negative counts', () => {
    expect(() => readPerformanceCaptures({ ...effect, dsp: { ...effect.dsp, unit: 'ms' } }, 'bad.json')).toThrow(
      'Invalid DSP block counters',
    );
    expect(() =>
      readPerformanceCaptures({ ...effect, dsp_rows: [{ ...effect.dsp_rows[0], sum_cycles: -1 }] }, 'bad.json'),
    ).toThrow('Invalid DSP block counters');
  });
});

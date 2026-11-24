import { describe, expect, it } from 'vitest';
import { readPerformanceCaptures } from '@digi/workbench-core/profile/performance-capture.ts';
import {
  focusTraceEvent,
  formatTraceTicks,
  traceBars,
  traceHit,
  traceRuler,
  traceTimeAt,
  traceTimeline,
  zoomTraceView,
} from '../src/trace-timeline';

describe('ColdFire trace timeline', () => {
  const timeline = traceTimeline({
    hz: 1_000_000,
    dropped: 0,
    events: [
      { owner: 2, startTicks: '18446744073709551508', durationTicks: 20 },
      { owner: 1, startTicks: '18446744073709551500', durationTicks: 5 },
      { owner: 2, startTicks: '18446744073709551504', durationTicks: 6 },
      { owner: 1, startTicks: '18446744073709551528', durationTicks: 0 },
    ],
  });

  it('subtracts the capture origin before projecting owner lanes', () => {
    expect(timeline.end).toBe(28);
    expect(timeline.lanes.map(({ owner }) => owner)).toEqual([1, 2]);
    const view = { start: 6, span: 10 };
    expect([...traceBars(timeline.lanes[1], view, 100)].map(({ left, width }) => [left, width])).toEqual([
      [0, 40],
      [20, 80],
    ]);
  });

  it('keeps the recording-relative origin and exact event start in the time labels', () => {
    expect(formatTraceTicks(timeline.origin, 1_000_000)).toBe('18446744073709.5515 s');
    const event = timeline.lanes[1].events[1];
    expect(formatTraceTicks(event.startTicks, 1_000_000)).toBe('18446744073709.551508 s');
    expect(formatTraceTicks(event.duration, 1_000_000)).toBe('20 µs');
  });

  it('hits the topmost overlapping bar and minimum-width pulses', () => {
    const view = { start: 0, span: 28 };
    expect(traceHit(timeline.lanes[1], view, 28, 9)?.start).toBe(8);
    expect(traceHit(timeline.lanes[1], view, 28, 5)?.start).toBe(4);
    expect(traceHit(timeline.lanes[0], view, 28, 9)).toBeUndefined();
    expect(traceHit(timeline.lanes[0], view, 28, 27.5)?.start).toBe(28);
  });

  it('can inspect a one-pixel call nearby without hiding direct hits', () => {
    const view = { start: 0, span: 28 };
    expect(traceHit(timeline.lanes[0], view, 28, 6, 3)?.start).toBe(0);
    expect(traceHit(timeline.lanes[0], view, 28, 9, 3)).toBeUndefined();
    expect(traceHit(timeline.lanes[1], view, 28, 9, 3)?.start).toBe(8);
  });

  it('zooms straight to a short event at a useful scale', () => {
    const [lane] = traceTimeline({
      hz: 1_000_000,
      dropped: 0,
      events: [
        { owner: 1, startTicks: '0', durationTicks: 1 },
        { owner: 1, startTicks: '5000', durationTicks: 5 },
      ],
    }).lanes;
    const view = focusTraceEvent(lane.events[1], 10000);
    expect(view.span).toBe(60);
    expect([...traceBars(lane, view, 600)][0]).toMatchObject({ left: 275, width: 50 });
  });

  it('uses round intervals on the ruler and keeps tick labels aligned after panning', () => {
    const ruler = traceRuler({ start: 0, span: 668_535_835 }, 0n, 132_000_000, 1200);
    expect(ruler.ticks.map(({ at }) => formatTraceTicks(at, 132_000_000, ruler.step / 10))).toEqual([
      '0 s',
      '1 s',
      '2 s',
      '3 s',
      '4 s',
      '5 s',
    ]);
    const zoomed = traceRuler({ start: 0.5, span: 10 }, 18446744073709551500n, 1_000_000, 480);
    expect(zoomed.ticks.map(({ at }) => at)).toEqual([18446744073709551505n, 18446744073709551510n]);
    expect(zoomed.ticks.map(({ fraction }) => fraction)).toEqual([0.45, 0.95]);
  });

  it('excludes positive-duration events touching only an outside edge', () => {
    const model = traceTimeline({
      hz: 1000,
      dropped: 0,
      events: [
        { owner: 1, startTicks: '0', durationTicks: 5 },
        { owner: 1, startTicks: '10', durationTicks: 2 },
      ],
    });
    expect([...traceBars(model.lanes[0], { start: 5, span: 5 }, 20)]).toEqual([]);
  });

  it('keeps time under the pointer when zooming and clamps to capture and tick limits', () => {
    const fit = { start: 0, span: 28 };
    const zoom = zoomTraceView(fit, 28, 0.75, 0.5);
    expect(zoom).toEqual({ start: 10.5, span: 14 });
    expect(traceTimeAt(zoom, 75, 100)).toBe(traceTimeAt(fit, 75, 100));
    expect(zoomTraceView(zoom, 28, 0.75, 100)).toEqual(fit);
    expect(zoomTraceView(zoom, 28, 0.75, 0.00001).span).toBe(1);
  });
});

it('keeps saved workload markers aligned with retained events in the same clock', () => {
  const [saved] = readPerformanceCaptures(
    {
      schema: 'dk.calcaptures/1',
      side: 'emu',
      captures: [
        { schema: 'dk.profile/1', run: { case_id: 'old' } },
        {
          schema: 'dk.profile/1',
          run: { id: 'emu-fx-1', case_id: 'fx' },
          spans: [{ owner: 256, kind: 'phase', name: 'early', calls: 1, sum_ticks: 5, max_ticks: 5 }],
          trace: {
            hz: 1000,
            dropped: 3,
            events: [{ owner: 256, startTicks: '20', durationTicks: 5 }],
            markers: [
              { name: 'Start', atTicks: '0' },
              { name: 'End', atTicks: '100' },
            ],
          },
        },
      ],
    },
    'run.json',
  );
  expect(saved.label).toBe('emu-fx-1');
  expect(saved.trace!.names.get(256)).toBe('early');
  expect(saved.trace!.trace.dropped).toBe(3);
  const timeline = traceTimeline(saved.trace!.trace);
  expect(timeline.end).toBe(100);
  expect(timeline.retained).toEqual({ start: 20, end: 25 });
  expect(timeline.lanes[0].events[0].start).toBe(20);
  expect(timeline.markers.map(({ offset }) => offset)).toEqual([0, 100]);
  expect(() => readPerformanceCaptures({ schema: 'dk.profile/1' }, 'no-trace.json')).toThrow('no profiling data');
});

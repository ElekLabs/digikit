import { describe, expect, it } from 'vitest';
import {
  formatTraceTicks,
  traceBars,
  traceHit,
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

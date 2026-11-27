import type { TraceCapture } from '@digi/workbench-core/profile/trace.ts';

interface TraceEvent {
  start: number;
  end: number;
  duration: number;
  startTicks: bigint;
}

interface TraceLane {
  owner: number;
  events: TraceEvent[];
  furthestEnd: number[];
}

export interface TraceView {
  start: number;
  span: number;
}

export function traceTimeline(capture: TraceCapture) {
  const lanes = new Map<number, TraceLane>();
  let origin: bigint | undefined;
  const parsed = capture.events.flatMap((event) => {
    if (!/^\d{1,20}$/.test(event.startTicks) || !Number.isSafeInteger(event.durationTicks) || event.durationTicks < 0)
      return [];
    const start = BigInt(event.startTicks);
    if (origin === undefined || start < origin) origin = start;
    return [{ ...event, start }];
  });

  const marks = (capture.markers ?? []).flatMap(({ name, atTicks }) => {
    if (!/^\d{1,20}$/.test(atTicks)) return [];
    const tick = BigInt(atTicks);
    if (origin === undefined || tick < origin) origin = tick;
    return [{ name, tick }];
  });
  const firstTick = origin ?? 0n;
  const markers = marks.map(({ name, tick }) => ({ name, tick, offset: Number(tick - firstTick) }));
  const markerEnd = markers.reduce((last, marker) => Math.max(last, marker.offset), 1);
  let eventStart = Number.POSITIVE_INFINITY;
  let eventEnd = 0;
  for (const { owner, start, durationTicks } of parsed) {
    let lane = lanes.get(owner);
    if (!lane) {
      lane = { owner, events: [], furthestEnd: [] };
      lanes.set(owner, lane);
    }

    // The capture clock is a decimal string and can already exceed the exact range of a number.
    const offset = start - firstTick;
    const finish = Number(offset + BigInt(durationTicks));
    lane.events.push({ start: Number(offset), end: finish, duration: durationTicks, startTicks: start });
    eventStart = Math.min(eventStart, Number(offset));
    eventEnd = Math.max(eventEnd, finish);
  }

  for (const lane of lanes.values()) {
    lane.events.sort((a, b) => a.start - b.start);
    let furthest = 0;
    for (const event of lane.events) {
      furthest = Math.max(furthest, event.end);
      lane.furthestEnd.push(furthest);
    }
  }
  const end = Math.max(markerEnd, eventEnd);
  const retained = parsed.length === 0 ? undefined : { start: eventStart, end: eventEnd };
  return { origin: firstTick, end, retained, markers, lanes: [...lanes.values()].sort((a, b) => a.owner - b.owner) };
}

export function moveTraceView(view: TraceView, end: number, start: number): TraceView {
  return { span: view.span, start: Math.max(0, Math.min(end - view.span, start)) };
}

export function minimumTraceSpan(end: number): number {
  return Math.max(1, end * Number.EPSILON * 4);
}

export function zoomTraceView(view: TraceView, end: number, anchor: number, factor: number): TraceView {
  const span = Math.min(end, Math.max(minimumTraceSpan(end), view.span * factor));
  return moveTraceView({ start: view.start, span }, end, view.start + (view.span - span) * anchor);
}

export function focusTraceEvent(event: TraceEvent, end: number): TraceView {
  const span = Math.min(end, Math.max(minimumTraceSpan(end), event.duration) * 12);
  return moveTraceView({ start: 0, span }, end, (event.start + event.end - span) / 2);
}

export function traceRuler(view: TraceView, origin: bigint, hz: number, width: number) {
  // Use round time intervals, keeping the first tick aligned to the capture clock.
  const target = view.span / hz / Math.max(1, Math.floor(width / 120));
  const magnitude = 10 ** Math.floor(Math.log10(target));
  const step = Math.max(1, Math.round(([1, 2, 5, 10].find((n) => n * magnitude >= target) ?? 10) * magnitude * hz));
  const wholeStart = Math.floor(view.start);
  const subTick = view.start - wholeStart;
  const start = origin + BigInt(wholeStart);
  const interval = BigInt(step);
  const firstWhole = start + BigInt(Math.ceil(subTick));
  const first = ((firstWhole + interval - 1n) / interval) * interval;
  const ticks = [];
  for (let at = first; ; at += interval) {
    const offset = Number(at - start) - subTick;
    if (offset > view.span) break;
    ticks.push({ at, fraction: offset / view.span });
  }
  return { step, ticks };
}

export function traceTimeAt(view: TraceView, x: number, width: number): number {
  return view.start + (x / width) * view.span;
}

function firstCandidate(lane: TraceLane, start: number) {
  let lo = 0;
  let hi = lane.events.length;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (lane.furthestEnd[mid] < start) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function* traceBars(lane: TraceLane, view: TraceView, width: number, hit?: { x: number; tolerance: number }) {
  const end = view.start + view.span;
  const first = hit ? Math.max(view.start, traceTimeAt(view, hit.x - hit.tolerance - 1, width)) : view.start;
  // A pulse can use the rightmost pixel even when it starts exactly at the end of the view.
  const last = !hit || hit.x + hit.tolerance >= width - 1 ? end : traceTimeAt(view, hit.x + hit.tolerance, width);
  for (let i = firstCandidate(lane, first); i < lane.events.length; i++) {
    const event = lane.events[i];
    if (event.start > last) break;
    if (event.end < first || (event.duration > 0 && (event.end <= view.start || event.start >= end))) continue;
    const left = Math.max(0, Math.min(width - 1, ((event.start - view.start) / view.span) * width));
    const right = Math.min(width, ((event.end - view.start) / view.span) * width);
    yield { event, left, width: Math.max(1, right - left) };
  }
}

export function traceHit(lane: TraceLane, view: TraceView, width: number, x: number, tolerance = 0) {
  if (x < 0 || x >= width) return;

  // Prefer a direct hit, then the nearest bar. Later bars paint over earlier ones.
  let nearest: TraceEvent | undefined;
  let best = tolerance;
  for (const bar of traceBars(lane, view, width, { x, tolerance })) {
    const right = bar.left + bar.width;
    if (bar.left <= x && x < right) {
      nearest = bar.event;
      best = 0;
    } else {
      const distance = Math.max(bar.left - x, x - right);
      if (best > 0 && distance <= best) {
        nearest = bar.event;
        best = distance;
      }
    }
  }
  return nearest;
}

export function formatTraceTicks(ticks: number | bigint, hz: number, resolution = 1): string {
  const seconds = Number(ticks) / hz;
  const [scale, unit]: [number, string] =
    seconds === 0 || seconds >= 1
      ? [1, 's']
      : seconds >= 1e-3
        ? [1e3, 'ms']
        : seconds >= 1e-6
          ? [1e6, 'µs']
          : [1e9, 'ns'];
  const digits = Math.max(0, Math.min(9, Math.ceil(-Math.log10((Math.max(1, resolution) / hz) * scale))));
  const trim = (value: string) => value.replace(/0+$/, '').replace(/\.$/, '');
  if (!Number.isSafeInteger(hz)) {
    const value = (seconds * scale).toFixed(digits);
    return `${digits ? trim(value) : value} ${unit}`;
  }

  const exactTicks = typeof ticks === 'bigint' ? ticks : BigInt(Math.round(ticks));
  const clock = BigInt(hz);
  const factor = BigInt(scale) * 10n ** BigInt(digits);
  const rounded = ((exactTicks * factor + clock / 2n) / clock).toString().padStart(digits + 1, '0');
  return `${digits ? trim(`${rounded.slice(0, -digits)}.${rounded.slice(-digits)}`) : rounded} ${unit}`;
}

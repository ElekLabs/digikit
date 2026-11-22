export interface TraceTimelineCapture {
  hz: number;
  dropped: number;
  events: readonly { owner: number; startTicks: string; durationTicks: number }[];
}

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

export function traceTimeline(capture: TraceTimelineCapture) {
  const lanes = new Map<number, TraceLane>();
  let origin: bigint | undefined;
  const parsed = capture.events.flatMap((event) => {
    if (!/^\d{1,20}$/.test(event.startTicks) || !Number.isSafeInteger(event.durationTicks) || event.durationTicks < 0)
      return [];
    const start = BigInt(event.startTicks);
    if (origin === undefined || start < origin) origin = start;
    return [{ ...event, start }];
  });

  const firstTick = origin ?? 0n;
  let end = 1;
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
    end = Math.max(end, finish);
  }

  for (const lane of lanes.values()) {
    lane.events.sort((a, b) => a.start - b.start);
    let furthest = 0;
    for (const event of lane.events) {
      furthest = Math.max(furthest, event.end);
      lane.furthestEnd.push(furthest);
    }
  }
  return { origin: firstTick, end, lanes: [...lanes.values()].sort((a, b) => a.owner - b.owner) };
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

export function* traceBars(lane: TraceLane, view: TraceView, width: number, atX?: number) {
  const end = view.start + view.span;
  const first = atX === undefined ? view.start : Math.max(view.start, traceTimeAt(view, atX - 1, width));
  // A pulse can use the rightmost pixel even when it starts exactly at the end of the view.
  const last = atX === undefined || atX >= width - 1 ? end : traceTimeAt(view, atX, width);
  for (let i = firstCandidate(lane, first); i < lane.events.length; i++) {
    const event = lane.events[i];
    if (event.start > last) break;
    if (event.end < first || (event.duration > 0 && (event.end <= view.start || event.start >= end))) continue;
    const left = Math.max(0, Math.min(width - 1, ((event.start - view.start) / view.span) * width));
    const right = Math.min(width, ((event.end - view.start) / view.span) * width);
    yield { event, left, width: Math.max(1, right - left) };
  }
}

export function traceHit(lane: TraceLane, view: TraceView, width: number, x: number) {
  if (x < 0 || x >= width) return;

  // Later bars paint on top of earlier ones in the same lane.
  let hit: TraceEvent | undefined;
  for (const bar of traceBars(lane, view, width, x)) {
    if (bar.left <= x && x < bar.left + bar.width) hit = bar.event;
  }
  return hit;
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

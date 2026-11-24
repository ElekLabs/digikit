export interface TraceMarker {
  name: string;
  atTicks: string;
}

export interface TraceCapture {
  hz: number;
  dropped: number;
  events: readonly { owner: number; startTicks: string; durationTicks: number }[];
  markers?: readonly TraceMarker[];
}

export interface NamedTrace {
  trace: TraceCapture;
  names: ReadonlyMap<number, string>;
}

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
const unsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
const ticks = (value: unknown): value is string =>
  typeof value === 'string' && /^\d{1,20}$/.test(value) && BigInt(value) <= 0xffff_ffff_ffff_ffffn;

export function readTrace(doc: Record<string, unknown>): NamedTrace | undefined {
  const trace = doc.trace;
  if (trace === undefined) return;
  if (
    !object(trace) ||
    !unsigned(trace.hz) ||
    trace.hz === 0 ||
    !unsigned(trace.dropped) ||
    !Array.isArray(trace.events)
  )
    throw new Error('Invalid ColdFire trace clock or event list.');
  const events = trace.events.map((row) => {
    if (
      !object(row) ||
      !unsigned(row.owner) ||
      row.owner > 0xffff_ffff ||
      !ticks(row.startTicks) ||
      !unsigned(row.durationTicks)
    )
      throw new Error('Invalid ColdFire trace event.');
    return { owner: row.owner, startTicks: row.startTicks, durationTicks: row.durationTicks };
  });
  const markers = trace.markers;
  if (markers !== undefined && !Array.isArray(markers)) throw new Error('Invalid ColdFire trace marker list.');
  const names = new Map<number, string>();
  if (Array.isArray(doc.spans)) {
    for (const row of doc.spans) {
      if (object(row) && unsigned(row.owner) && typeof row.name === 'string' && row.name)
        names.set(row.owner, row.name);
    }
  }
  return {
    trace: {
      hz: trace.hz,
      dropped: trace.dropped,
      events,
      markers: markers?.map((marker) => {
        if (!object(marker) || typeof marker.name !== 'string' || !marker.name || !ticks(marker.atTicks))
          throw new Error('Invalid ColdFire trace marker.');
        return { name: marker.name, atTicks: marker.atTicks };
      }),
    },
    names,
  };
}

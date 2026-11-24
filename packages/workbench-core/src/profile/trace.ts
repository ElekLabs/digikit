import { expandCaptures } from './calibration-view.ts';

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

export interface SavedTrace {
  label: string;
  trace: TraceCapture;
  names: ReadonlyMap<number, string>;
}

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
const unsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
const ticks = (value: unknown): value is string =>
  typeof value === 'string' && /^\d{1,20}$/.test(value) && BigInt(value) <= 0xffff_ffff_ffff_ffffn;

function savedTrace(doc: unknown, label: string): SavedTrace | undefined {
  if (!object(doc) || doc.schema !== 'dk.profile/1')
    throw new Error('Expected a dk.profile/1 capture or a calibration capture set.');
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
  const id = object(doc.run) && typeof doc.run.id === 'string' && doc.run.id;
  return {
    label: id || label,
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

/** Accept a Profile export or a saved calibration run. A set may include captures without a trace. */
export function readSavedTraces(doc: unknown, filename: string): SavedTrace[] {
  const traces = expandCaptures(doc, undefined, filename).flatMap((capture, index) => {
    const found = savedTrace(capture, index ? filename + ' · ' + (index + 1) : filename);
    return found ? [found] : [];
  });
  if (!traces.length) throw new Error('This capture has no retained ColdFire trace.');
  return traces;
}

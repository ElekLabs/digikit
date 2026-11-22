import {
  ownerName,
  shareOf,
  type ModuleRef,
  type Snapshot,
  type SpanIn,
} from '@digi/workbench-core/profile/profile-core.ts';

const WINDOW_MS = 60_000;
const MAX_GAP_MS = 5_000;

export interface SpanInterval {
  start: number;
  end: number;
  rows: { id: string; name: string; calls: number; share: number }[];
}

interface SpanIntervals {
  /** Add a reading at the host time in milliseconds. Shares are based on the target's elapsed ticks. */
  push(at: number, snap: Snapshot, modules: ModuleRef[]): void;
  intervals(): readonly SpanInterval[];
  /** Skip the next delta while retaining the intervals already observed. */
  break(): void;
  clear(): void;
}

interface Baseline {
  at: number;
  elapsedTicks: bigint;
  owners: string;
  spans: Map<string, SpanIn>;
}

function ownerLayout(snap: Snapshot, modules: ModuleRef[]): string {
  const records = [...modules].sort((a, b) => a.index - b.index).map((m) => [m.index, m.name, m.kind]);
  // A symbol becoming available changes the label, while the function address and owner identify the poll.
  const polls = snap.polls?.flatMap((p, i) => (p ? [[i, p.owner, p.fn]] : [])) ?? [];
  return JSON.stringify([snap.hz, records, polls]);
}

export function createSpanIntervals(): SpanIntervals {
  let baseline: Baseline | undefined;
  let intervals: SpanInterval[] = [];
  let wasStopped = false;

  return {
    push(at, snap, modules) {
      const previous = baseline;
      const current: Baseline = {
        at,
        elapsedTicks: snap.elapsedTicks,
        owners: ownerLayout(snap, modules),
        spans: new Map(snap.spans.map((s) => [`${s.kind}:${s.index}`, { ...s }])),
      };
      // A stopped reading can close the previous interval. Later idle reads cannot become a baseline.
      baseline = snap.running ? current : undefined;
      if ((wasStopped && snap.running) || (previous && snap.elapsedTicks < previous.elapsedTicks)) intervals = [];
      wasStopped = !snap.running;

      // Use the interval's end for retention so the graph can clip one that crosses the window edge.
      intervals = intervals.filter((i) => i.end > at - WINDOW_MS);
      if (!previous || current.owners !== previous.owners || current.spans.size !== previous.spans.size) return;

      const elapsed = snap.elapsedTicks - previous.elapsedTicks;
      const rows: (SpanInterval['rows'][number] & { ticks: bigint })[] = [];
      for (const [id, s] of current.spans) {
        const old = previous.spans.get(id);
        if (!old || old.row !== s.row) return;
        const calls = s.calls - old.calls;
        const ticks = s.sumTicks - old.sumTicks;
        if (calls < 0 || ticks < 0n || s.maxTicks < old.maxTicks) {
          intervals = [];
          return;
        }
        if (calls || ticks)
          rows.push({ ticks, id, name: ownerName(s, modules, snap.polls), calls, share: shareOf(ticks, elapsed) });
      }

      const gap = at - previous.at;
      if (elapsed > 0n && gap > 0 && gap <= MAX_GAP_MS) {
        rows.sort((a, b) => (a.ticks === b.ticks ? a.id.localeCompare(b.id) : a.ticks > b.ticks ? -1 : 1));
        intervals.push({
          start: previous.at,
          end: at,
          rows: rows.map(({ id, name, calls, share }) => ({ id, name, calls, share })),
        });
      }
    },
    intervals: () => intervals,
    break() {
      baseline = undefined;
    },
    clear() {
      baseline = undefined;
      intervals = [];
      wasStopped = false;
    },
  };
}

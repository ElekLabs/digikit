/** A capture-wide summary of main CPU spans, never an interval series. */
export interface SpanSummary {
  readonly elapsedTicks?: number;
  readonly clockHz?: number;
  readonly rows: readonly SpanTotal[];
}

export interface SpanTotal {
  readonly name: string;
  readonly owner?: number;
  readonly kind?: string;
  readonly calls?: number;
  readonly totalTicks?: number;
  readonly maxTicks?: number;
  readonly totalUs?: number;
  readonly maxUs?: number;
  readonly share?: number;
}

function counter(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}

export function readSpanSummary(profile: Record<string, unknown>): SpanSummary | undefined {
  if (!Array.isArray(profile.spans)) return undefined;
  const elapsedTicks = counter(profile.span_elapsed_ticks);
  const clockHz = counter(profile.span_clock_hz);
  const toUs = (ticks?: number) => (clockHz && ticks !== undefined ? (ticks / clockHz) * 1e6 : undefined);
  const rows = profile.spans.flatMap((row): SpanTotal[] => {
    if (!row || typeof row.name !== 'string' || !row.name) return [];
    const totalTicks = counter(row.sum_ticks);
    const maxTicks = counter(row.max_ticks);
    return [
      {
        name: row.name,
        owner: counter(row.owner),
        kind: typeof row.kind === 'string' ? row.kind : undefined,
        calls: counter(row.calls),
        totalTicks,
        maxTicks,
        totalUs: toUs(totalTicks),
        maxUs: toUs(maxTicks),
        share: elapsedTicks && totalTicks !== undefined ? totalTicks / elapsedTicks : undefined,
      },
    ];
  });
  if (!rows.length) return undefined;
  rows.sort((a, b) => (b.totalTicks ?? -1) - (a.totalTicks ?? -1));
  return { elapsedTicks, clockHz, rows };
}

export function formatSpanShare(share?: number): string {
  if (share === undefined) return '—';
  const percent = share * 100;
  return percent > 0 && percent < 0.1 ? '<0.1%' : `${percent.toFixed(1)}%`;
}

import { expandCaptures } from './calibration-view.ts';
import { readDspBlocks } from './dsp-blocks.ts';
import { readProfileHotspots } from './hotspots.ts';
import { readSpanSummary, type SpanSummary } from './span-summary.ts';
import { readTrace } from './trace.ts';

export interface PerformanceCapture {
  label: string;
  document: Record<string, unknown>;
  hasHotspots: boolean;
  spans?: SpanSummary;
  trace?: ReturnType<typeof readTrace>;
  dsp?: ReturnType<typeof readDspBlocks>;
}

export function readPerformanceCaptures(doc: unknown, filename: string): PerformanceCapture[] {
  const captures = expandCaptures(doc, undefined, filename).flatMap((entry, index) => {
    if (!entry || typeof entry !== 'object' || !('schema' in entry) || entry.schema !== 'dk.profile/1')
      throw new Error('Expected a dk.profile/1 capture or a calibration capture set.');
    const profile = entry as Record<string, unknown>;
    const trace = readTrace(profile);
    const dsp = readDspBlocks(profile);
    const spans = readSpanSummary(profile);
    const { pcs } = readProfileHotspots(profile, 1);
    const hasHotspots = !!(pcs.cf.length || pcs.sharc.length);
    if (!trace && !dsp && !hasHotspots && !spans) return [];
    const run = profile.run;
    const info = run && typeof run === 'object' ? (run as { label?: unknown; id?: unknown }) : undefined;
    const label = typeof info?.label === 'string' ? info.label : typeof info?.id === 'string' ? info.id : undefined;
    return [
      {
        label: label || (index ? filename + ' · ' + (index + 1) : filename),
        document: profile,
        hasHotspots,
        spans,
        trace,
        dsp,
      },
    ];
  });
  if (!captures.length) throw new Error('This capture has no profiling data.');
  return captures;
}

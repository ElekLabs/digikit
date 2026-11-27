import type { DspHeader, DspRowOut } from './profile-artifact.ts';

export interface DspBlockCapture {
  unit: 'cycles' | 'instructions';
  blocks: number;
  overruns: number;
  rows: {
    row: number;
    kind: 'machine' | 'effect';
    module?: string;
    route?: 'send' | 'master';
    calls: number;
    meanBlock: number;
    maxBlock: number;
    maxCall: number;
  }[];
}

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
const counter = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0 && Number.isInteger(value);

export function dspBlockCapture(
  header: Pick<DspHeader, 'blocks' | 'overruns' | 'unit'>,
  rows: readonly DspRowOut[],
  dispatch: readonly unknown[] = [],
): DspBlockCapture | undefined {
  if (header.blocks === 0 || (header.unit !== 'insn' && header.unit !== 'cycles')) return undefined;
  return {
    unit: header.unit === 'insn' ? 'instructions' : 'cycles',
    blocks: header.blocks,
    overruns: header.overruns,
    rows: rows.map((row) => {
      const owner = dispatch.find(
        (entry): entry is Record<string, unknown> =>
          object(entry) && entry.row === row.row && entry.kind === row.kind && typeof entry.module === 'string',
      );
      const route = owner?.route;
      return {
        row: row.row,
        kind: row.kind,
        ...(typeof owner?.module === 'string' ? { module: owner.module } : {}),
        ...(route === 'send' || route === 'master' ? { route } : {}),
        calls: row.calls,
        meanBlock: row.sum_cycles / header.blocks,
        maxBlock: row.max_block,
        maxCall: row.max_call,
      };
    }),
  };
}

/** Use the saved header's unit and dispatch map, independent of the current Workbench target. */
export function readDspBlocks(doc: Record<string, unknown>): DspBlockCapture | undefined {
  if (doc.dsp === undefined && doc.dsp_rows === undefined) return undefined;
  const header = doc.dsp;
  if (
    !object(header) ||
    !counter(header.blocks) ||
    !counter(header.overruns) ||
    (header.unit !== 'insn' && header.unit !== 'cycles') ||
    !Array.isArray(doc.dsp_rows) ||
    !doc.dsp_rows.every(isRow)
  ) {
    throw new Error('Invalid DSP block counters.');
  }
  const dispatch = object(doc.run) && Array.isArray(doc.run.dispatch) ? doc.run.dispatch : [];
  return dspBlockCapture(
    { unit: header.unit, blocks: header.blocks, overruns: header.overruns },
    doc.dsp_rows,
    dispatch,
  );
}

function isRow(row: unknown): row is DspRowOut {
  return (
    object(row) &&
    (row.kind === 'machine' || row.kind === 'effect') &&
    counter(row.row) &&
    counter(row.calls) &&
    counter(row.sum_cycles) &&
    counter(row.max_call) &&
    counter(row.max_block)
  );
}

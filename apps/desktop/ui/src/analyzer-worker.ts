import type { AnalyzerInput, AnalyzerResult } from './analyzer-frame';
import { createAnalyzerMeasurement } from './analyzer-measurement';

const analyze = createAnalyzerMeasurement();
const worker = self as unknown as DedicatedWorkerGlobalScope;

worker.onmessage = ({ data }: MessageEvent<AnalyzerInput>) => {
  const frame = analyze(data);
  const arrays = [...frame.scope, ...(frame.spectrum ?? []), ...(frame.heldSpectrum ?? [])];
  const result: AnalyzerResult = { revision: data.revision, holdRevision: data.holdRevision, frame };
  worker.postMessage(
    result,
    arrays.map((samples) => samples.buffer),
  );
};

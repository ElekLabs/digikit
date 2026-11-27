import type { SamplesSlot } from '@digi/workbench-core/transfer/env.ts';
import SamplesMode from './SamplesMode';
import type { SamplesApp } from './app';

export function createSamplesSlot(samples: SamplesApp): SamplesSlot {
  return { ...samples.transfer, Mode: (props) => <SamplesMode ctx={props.ctx} samples={samples} /> };
}

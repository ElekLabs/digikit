// The Performance drawer observes and controls the selected Profile panel's recorder.
import type { ProfileTrace } from '@digi/dk-session';
import type { DspBlockCapture } from '@digi/workbench-core/profile/dsp-blocks.ts';
import type { SpanInterval } from './span-intervals';

export interface FrozenTrace {
  trace: ProfileTrace;
  names: ReadonlyMap<number, string>;
}

export interface SpanRecording {
  dspBlocks(): DspBlockCapture | undefined;
  dspBlockStatus(): string;
  frozenTrace(): FrozenTrace | undefined;
  timelineStatus(): string;
  intervals(): readonly SpanInterval[];
  running(): boolean;
  available(): boolean;
  disabled(): boolean;
  status(): string;
  includeDsp(): boolean;
  dspAvailable(): boolean;
  setIncludeDsp(value: boolean): void;
  includeDspHotspots(): boolean;
  dspHotspotsAvailable(): boolean;
  setIncludeDspHotspots(value: boolean): void;
  toggle(): void;
  canSave(): boolean;
  save(): Promise<void>;
  reset(): Promise<void>;
}

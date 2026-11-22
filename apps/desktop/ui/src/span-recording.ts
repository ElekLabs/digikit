// The Performance drawer observes and controls the selected Profile panel's recorder.
import type { SpanInterval } from './span-intervals';

export interface SpanRecording {
  intervals(): readonly SpanInterval[];
  running(): boolean;
  available(): boolean;
  disabled(): boolean;
  status(): string;
  toggle(): void;
}

// The Performance drawer observes and controls the selected Profile panel's recorder.
import type { ProfileTrace } from '@digi/dk-session';
import type { SpanInterval } from './span-intervals';

export interface FrozenTrace {
  trace: ProfileTrace;
  names: ReadonlyMap<number, string>;
}

export interface SpanRecording {
  frozenTrace(): FrozenTrace | undefined;
  timelineStatus(): string;
  intervals(): readonly SpanInterval[];
  running(): boolean;
  available(): boolean;
  disabled(): boolean;
  status(): string;
  toggle(): void;
}

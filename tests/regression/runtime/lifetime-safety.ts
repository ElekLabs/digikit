import { type Device } from '@digi/test-support';
import { expect } from 'vitest';
import { record } from './artifacts.ts';
import { runtimeSafety, type SafetyPolicy } from './write-safety.ts';

/** After the owned emulators exit: every watch log and exported overlay meets the safety policy of the card's services. One call site for every session kind. */
export function expectLifetimeSafety(run: {
  device: Device;
  watchOutput: string;
  overlays: string[];
  epochs: Set<number>;
  artifacts: string;
  policy: SafetyPolicy;
  cardServices: Iterable<string>;
}) {
  const { device, watchOutput, overlays, epochs, artifacts, policy, cardServices } = run;
  const report = runtimeSafety(device, watchOutput, overlays, epochs.size, policy, [], [...cardServices]);
  record(artifacts, 'lifetime-safety', report);
  expect(report.pass, 'every emulator and exported overlay meets the write-safety policy').toBe(true);
}

import type { Device, SessionOptions } from '@digi/test-support';
import { describe, test } from 'vitest';
import { expectRuntimeBoot } from '../runtime-boot.ts';
import { withPatchedSession } from '../patched-session.ts';
import type { TestRuntime } from '../test-runtime.ts';
import { timePhase, timingContext } from '../timing.ts';
import { FIRMWARE, FULL_TIER, selectDevices, skipEmptyHalf } from './devices.ts';
import { liveOptions } from './live-options.ts';

/** One test per device, each on its own fresh runtime daemon that has passed `expectRuntimeBoot`. */
export function runtimeTests(
  name: string,
  body: (runtime: TestRuntime) => Promise<void>,
  opts: {
    patchSet?: string;
    dsp?: SessionOptions['dsp'];
    mirror?: boolean;
    /** Leave the card to the daemon (the stock initialisation of a blank card), instead of the cached card of `liveCard`. */
    blankCard?: boolean;
    /** The stable period of the card in DSP blocks (default `LIVE_STABLE_BLOCKS`; 0 is the runtime's 30 s). */
    stableBlocks?: number;
    devices?: readonly Device[];
    full?: boolean;
    skip?: string;
  } = {},
): void {
  const selected = selectDevices(opts.devices);
  skipEmptyHalf(selected, name);
  for (const device of selected) {
    describe.skipIf((opts.full === true && !FULL_TIER) || opts.skip !== undefined)(
      `${device.toUpperCase()} ${FIRMWARE[device]}`,
      () => {
        const handle = withPatchedSession({ ...liveOptions(device, opts), dsp: opts.dsp }, name);
        test(name, async () => {
          await expectRuntimeBoot(handle.runtime);
          await timePhase('scenario.body', () => body(handle.runtime), timingContext(handle.runtime.session));
        });
      },
    );
  }
}

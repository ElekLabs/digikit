import { test } from 'vitest';
import { expectRuntimeBoot, startPatchedSession } from '../support.ts';
import { timePhase, timingContext } from '../timing.ts';
import { DEVICES, FIRMWARE, liveOptions, moduleArtifact } from '../runtime/live.ts';
import { expectModuleLoad, type RuntimeModule } from './acceptance.ts';

// Every cell owns a fresh coupled daemon, card and module selection. No patched restart/reset pooling.
export function moduleLoadTests(module: RuntimeModule): void {
  for (const device of DEVICES) {
    const name = `${device} ${FIRMWARE[device]} loads and registers ${module.id} through USB without runtime faults`;
    test(name, async () => {
      const options = liveOptions(device, { mirror: true });
      const artifact = moduleArtifact(device, module.id);
      const runtime = await startPatchedSession(options, name);
      try {
        await expectRuntimeBoot(runtime);
        await timePhase(
          'scenario.body',
          () => expectModuleLoad(runtime, module, artifact),
          timingContext(runtime.session),
        );
      } finally {
        await runtime.close();
      }
    });
  }
}

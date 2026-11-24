import { onCleanup } from 'solid-js';
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { createEmulatorActions } from '../emulator-actions';
import { createEmulatorFirmware } from './firmware';
import { createEmulatorSession } from './session';

type WorkbenchEmulatorOptions = Omit<Parameters<typeof createEmulatorFirmware>[0], 'emu' | 'runtime' | 'signal'> & {
  host: Pick<Host, 'emu' | 'rpc'>;
  actions: Pick<ReturnType<typeof createEmulatorActions>, 'registerReloadWithRuntime' | 'registerFlashUi'>;
  pollAudio: () => Promise<void>;
  pollHealth: () => Promise<void>;
};

/** Connect the native session and firmware commands under the Workbench's Solid owner. */
export function createWorkbenchEmulator(options: WorkbenchEmulatorOptions) {
  const { host, actions, pollAudio, pollHealth, ...firmwareOptions } = options;
  // Native startup and the polls run on mount, once both owners have been constructed.
  const session = createEmulatorSession({
    host,
    update: (event) => firmware.update(event),
    applyLaunch: (launch) => firmware.applyLaunch(launch),
    pollSource: () => firmware.pollSource(),
    pollAudio,
    pollHealth,
  });
  const firmware = createEmulatorFirmware({
    ...firmwareOptions,
    emu: host.emu,
    runtime: session.runtime,
    signal: session.signal,
  });
  onCleanup(actions.registerReloadWithRuntime(firmware.reloadWithRuntime));
  onCleanup(actions.registerFlashUi(firmware));
  return { ...session, firmware };
}

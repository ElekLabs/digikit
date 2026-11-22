// A controller snapshot for the real native runtime and the live faceplate.
import { deviceProfiles, deviceOfPort } from '@digi/workbench-core/data/device-profiles.ts';
import type { RuntimeSnapshot } from '@digi/workbench-core/runtime/runtime.ts';
import { cardDevice } from './card-mock';
import { queryFrame } from './frame-from-png';
import { testPattern } from './remote-mock';

export function previewEmulator(query: URLSearchParams) {
  const bootFile = query.get('bootfile') ?? 'Digitakt_II_OS1.17.syx';
  const bootDevice = deviceOfPort(bootFile.replaceAll('_', ' '));
  const device =
    query.get('bootdevice') ??
    query.get('emudev') ??
    (query.has('boot') || query.get('page') === 'run' ? bootDevice : undefined) ??
    cardDevice;
  const release = deviceProfiles[device]?.firmware.at(-1);
  const name = query.has('boot') ? bootFile : (release?.filename ?? device);
  const pending = query.get('page') === 'run' || query.get('boot') === 'loading';
  const failed = query.get('boot') === 'failed';
  const frame = queryFrame().then((value) => value ?? testPattern());
  const snapshot = async (): Promise<RuntimeSnapshot> => ({
    polled: true,
    frame: await frame,
    status: {
      device,
      version: release?.version ?? '',
      icount: 100000,
      pc: 0,
      ready: true,
      phase: 'running',
      error: null,
      frame_revision: 1,
      frame_source: 'screen',
      main_ui_reached: true,
      filesystem_verified: true,
      input_ready: true,
      input_pending: 0,
      input_irqs: 0,
    },
  });
  return {
    source: { name, path: name, device },
    launch: pending || failed ? { syx: name } : { loaded: true },
    startup() {
      if (pending || failed) throw new Error('No startup firmware selected');
      return snapshot();
    },
    snapshot,
    async load() {
      if (failed) throw new Error('the emulator did not answer within 120 s');
      if (pending) await new Promise<void>(() => {});
    },
  };
}

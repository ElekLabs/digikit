// A controller snapshot for the real native runtime and the live faceplate.
import { deviceProfiles } from '@digi/workbench-core/data/device-profiles.ts';
import type { RuntimeSnapshot } from '@digi/workbench-core/runtime/runtime.ts';
import { previewBootFile, previewEmulatorDevice } from './card-mock';
import { createPreviewScreen } from './preview-screen';

export function previewEmulator(query: URLSearchParams) {
  const bootFile = previewBootFile(query);
  const device = previewEmulatorDevice(query);
  const release = deviceProfiles[device]?.firmware.at(-1);
  const name = query.has('boot') ? bootFile : (release?.filename ?? device);
  const pending = query.get('page') === 'run' || query.get('boot') === 'loading';
  const failed = query.get('boot') === 'failed';
  let loaded = query.size > 0 && !pending && !failed;
  let patched = true;
  let dsp = true;
  const screen = createPreviewScreen(device);
  let sentRevision = 0;
  const snapshot = async (includeFrame = true): Promise<RuntimeSnapshot> => {
    const revision = screen.revision();
    const changed = sentRevision !== revision;
    sentRevision = revision;
    return {
      polled: true,
      ...(includeFrame || changed ? { frame: await screen.read() } : {}),
      status: {
        device,
        version: release?.version ?? '',
        icount: 100000,
        pc: 0,
        ready: true,
        phase: 'running',
        error: null,
        frame_revision: revision,
        frame_source: 'screen',
        main_ui_reached: true,
        filesystem_verified: true,
        input_ready: true,
        input_pending: 0,
        input_irqs: 0,
      },
    };
  };
  return {
    screen,
    source: { name, path: name, device },
    isLoaded: () => loaded,
    hasRuntime: () => loaded && patched,
    status: () => ({
      firmware: loaded ? { runtime: patched ? { patched: true, set: 'core/dynreg' } : null } : null,
      dsp_enabled: dsp,
    }),
    launch: pending || failed ? { syx: name } : { loaded },
    startup() {
      if (!loaded) throw new Error('No startup firmware selected');
      return snapshot();
    },
    snapshot,
    async load(runtime: boolean, enableDsp: boolean) {
      if (failed) throw new Error('the emulator did not answer within 120 s');
      if (pending) await new Promise<void>(() => {});
      loaded = true;
      patched = runtime;
      dsp = enableDsp;
    },
  };
}

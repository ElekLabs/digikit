// A controller snapshot for the real native runtime and the live faceplate.
import { deviceProfiles } from '@digi/workbench-core/data/device-profiles.ts';
import type { ControllerStatus, RuntimeSnapshot } from '@digi/workbench-core/runtime/runtime.ts';
import { previewBootFile, previewEmulatorDevice } from './card-mock';
import { createPreviewScreen } from './preview-screen';

function previewLoad(seconds: number, dspEnabled: boolean): NonNullable<ControllerStatus['load']> {
  const cf = 32 + 6 * Math.sin(seconds / 4);
  const dsp = 48 + 9 * Math.sin(seconds / 7);
  return {
    window_s: 1,
    cf: { model_pct: cf, real_pct: cf, real_high_pct: cf + 4 },
    dsp: dspEnabled ? { busy_per_block: 18_000, model_pct: dsp, real_pct: dsp, real_high_pct: dsp + 6 } : null,
  };
}

export function previewEmulator(query: URLSearchParams) {
  const bootFile = previewBootFile(query);
  const device = previewEmulatorDevice(query);
  const release = deviceProfiles[device]?.firmware.at(-1);
  const name = query.has('boot') ? bootFile : (release?.filename ?? device);
  const pending = query.get('page') === 'run' || query.get('boot') === 'loading';
  const failed = query.get('boot') === 'failed';
  let loaded = !pending && !failed && query.get('boot') !== 'empty';
  let startedAt = performance.now();
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
      load: loaded ? previewLoad((performance.now() - startedAt) / 1000, dsp) : null,
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
      startedAt = performance.now();
      loaded = true;
      patched = runtime;
      dsp = enableDsp;
    },
  };
}

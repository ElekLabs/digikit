// What the Calibration section of the Profile pane does (CalibrationSection.tsx): play a calibration profile on the pane's target through the shared
// runner (workbench-core/profile/calibration-run.ts, the code `wb profile calibrate run` plays) and fit a device run against an emulator run with
// dkprof.wasm (`calibrateFit`). No module state: the session, the controller's rpc, the Transfer app pieces and the dkprof loader are arguments, so a
// test gives fakes. The preview replaces this module (preview/calibration-backend.mock.ts).
import type { DkSession, MidiSink } from '@digi/dk-session';
import { emuMidiSink } from '@digi/dk-session';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { profileTracks } from '@digi/workbench-core/calibration/runner.ts';
import type { DkProf } from '@digi/dk-session/dkprof';
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { LibraryModule } from '@digi/workbench-core/modules/dktransfer.ts';
import type { CalProfile } from '@digi/workbench-core/calibration/calprofile.ts';
import {
  deviceOfInfo,
  runCalibrationOnTarget,
  type RunOptions,
  type RunOutcome,
} from '@digi/workbench-core/profile/calibration-run.ts';
import {
  estimateSeconds,
  fitRequest,
  loadCalibrationFiles,
  type CalFileTexts,
  type CalibrationDoc,
  type CaptureSet,
} from '@digi/workbench-core/profile/calibration-view.ts';

export interface CalibrationDeps {
  session: () => Promise<DkSession>;
  /** The target is the emulator: the controller's rpc (MIDI input and its drain), the emulated session clock. */
  rpc?: Host['rpc'];
  /** The host's MIDI port, the sink of a device target. */
  midi?: MidiSink;
  /** The built modules of a device id (`TransferApp.loadLibrary`). */
  loadLibrary: (device: string) => Promise<LibraryModule[]>;
  /**
   * The Transfer install of modules on the selected target (`TransferApp.backend.install`). Absent when the pane's session is not the selected
   * target: a case that loads modules is then skipped with its reason.
   */
  install?: (ids: string[]) => Promise<void>;
  prof: () => Promise<DkProf>;
  texts: () => CalFileTexts;
}

export interface ProfileChoice {
  name: string;
  description: string;
  /** How long a run takes before it starts (`estimateSeconds`): captures times (warm-up and window), plus a restart for each case that changes the modules. */
  seconds: number;
  /** The profile with its module cases (the pane's time left counts its cases). */
  profile: CalProfile;
}

export interface Calibrator {
  profiles(): ProfileChoice[];
  /** The tracks (1 to 16) the profile uses on the pane's target: a run puts them on the stock machine to start and leaves them there. */
  tracks?(name: string): Promise<number[]>;
  run(opts: RunOptions): Promise<RunOutcome>;
  fit(emu: CaptureSet, dev: CaptureSet): Promise<CalibrationDoc>;
}

export function createCalibrator(deps: CalibrationDeps): Calibrator {
  let loaded: ReturnType<typeof loadCalibrationFiles> | undefined;
  const files = () => (loaded ??= loadCalibrationFiles(deps.texts()));
  return {
    profiles: () =>
      files().profiles.map((p) => ({
        name: p.name,
        description: p.description,
        seconds: estimateSeconds(p),
        profile: p,
      })),
    async tracks(name) {
      const profile = files().profiles.find((p) => p.name === name);
      const midi = registry.get(deviceOfInfo((await (await deps.session()).info()).port)).midi;
      return profile && midi ? profileTracks(profile, files().steps, midi) : [];
    },
    async run(opts) {
      const session = await deps.session();
      const emulator = !!deps.rpc;
      const install = deps.install;
      const sink = deps.rpc ? emuMidiSink((m, p) => deps.rpc!.call(m, p)) : deps.midi;
      if (!sink) throw new Error('no MIDI port to play the notes on');
      return runCalibrationOnTarget(
        {
          session,
          emulator,
          sink,
          ...(deps.rpc ? { call: <T>(m: string, p?: Record<string, unknown>) => deps.rpc!.call<T>(m, p) } : {}),
          files: files(),
          ...(install
            ? {
                modules: {
                  library: async () =>
                    (await deps.loadLibrary((await session.info()).port)).map((m) => ({
                      id: m.id,
                      path: m.path,
                      bytes: m.bytes,
                    })),
                  install,
                },
              }
            : {}),
        },
        opts,
      );
    },
    async fit(emu, dev) {
      return (await deps.prof()).calibrateFit(fitRequest(emu, dev)) as CalibrationDoc;
    },
  };
}

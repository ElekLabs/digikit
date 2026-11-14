// The desktop workbench's side of the Transfer pane (apps/web/src/transfer/env.ts): the library folder of built modules, the MIDI ports and
// the emulator through the Host (host/host.ts), the samples mode and the Card panel. main.tsx builds it with `createDesktopApps`.
import { createComponent } from 'solid-js';
import { EmuBulkLink } from '@digi/dk-session';
import { listingChanged } from '../../../web/src/transfer/refresh-core';
import type { TransferApp } from '../../../web/src/transfer/app';
import type { SamplesSlot, TransferEnv } from '../../../web/src/transfer/env';
import { MODULE_FILES } from '../../../web/src/transfer/module-files.generated';
import { targetLabel, type Target } from '../../../web/src/transfer/target';
import { failureLine } from '../../../web/src/transfer/wait-core';
import { bumpCardRefresh } from './card-state';
import { reloadWithRuntime } from './emulator-actions';
import { TauriBulkLink } from './bulk-link';
import { EmuUsbTransport, emulatorMode } from './desktop-transport';
import { logInput } from './log-state';
import { transferEntry } from './log-core';
import type { Host, LibBuildReport } from './host/host';
import { MidiPortTransport } from './midi-transport';
import { createSamplesBackend, type SamplesBackendApi } from './samples-backend';
import { samplesBar, uploadLines } from './samples-core';
import {
  localEntries,
  localFolder,
  setSampleEntries,
  setSampleFolder,
  setSampleTicks,
  setUploads,
  uploadBase,
  uploadAverage,
  uploadNotes,
  uploads,
} from './samples-state';
import SamplesMode from './SamplesMode';
import { readSamplePanes } from './samples-read';

export const PAUSED =
  'The emulator is paused: nothing would run the transfer. Resume it (the Run button of the workbench), then try again.';

/** The Samples slot of the pane. `app` and `backend` are functions: the app is built from the environment that holds this slot. */
function samplesSlot(app: () => TransferApp, backend: () => SamplesBackendApi): SamplesSlot {
  return {
    Mode: (p) =>
      createComponent(SamplesMode, {
        get ctx() {
          return p.ctx;
        },
        get backend() {
          return backend();
        },
      }),
    readLocal: () => backend().readLocal(),
    async read() {
      const { readLocal, readSamples } = backend();
      await readSamplePanes(readLocal, readSamples, (e) => failureLine(e, targetLabel(app().target.target())));
    },
    reset() {
      setSampleEntries(undefined);
      setSampleFolder('');
      setSampleTicks(new Set<string>());
      setUploads({});
      app().state.setSampleLink(undefined);
    },
    bar(busy) {
      const info = samplesBar(uploads(), uploadBase(), busy === 'Uploading', uploadAverage(), app().state.sampleLink());
      return info ? { info, running: busy === 'Uploading' } : undefined;
    },
    lines: () => [...uploadNotes(), ...uploadLines(uploads(), uploadBase())],
    async localChanged() {
      if (!localFolder()) return false;
      const now = await backend().listLocalNow();
      return !!now && listingChanged(localEntries(), now);
    },
  };
}

/** One entry per module that did not build, one for what changed; nothing when all is unchanged. */
function logBuild(device: string, report: LibBuildReport): void {
  for (const f of report.failed)
    logInput({
      source: 'transfer',
      level: 'error',
      text: `module ${f.id} did not build for ${device}`,
      detail: f.error,
    });
  const { changed, removed, ms } = report;
  if (changed.length || removed.length) {
    const detail = [...changed.map((id) => `packed ${id}`), ...removed.map((id) => `removed ${id}`)].join('\n');
    logInput({
      source: 'transfer',
      level: 'info',
      text: `${device} library: ${changed.length} packed new, ${removed.length} removed (${ms} ms)`,
      detail,
    });
  }
}

/**
 * The Transfer environment of the desktop app over `host`. `app` is the app that will own it (the samples backend and the bulk link use its
 * target and page state); it is a function because the app is built from the environment.
 */
export function createDesktopEnv(host: Host, app: () => TransferApp): TransferEnv {
  let backend: SamplesBackendApi | undefined;
  const samples = (): SamplesBackendApi => (backend ??= createSamplesBackend(host, app()));
  const emulatorBulk = EmuBulkLink.opener((method, params) => host.rpc.call(method, params));
  const builds = new Map<string, Promise<void>>();
  /** Build and pack the device's run-time modules (`lib_build`) before the library is read; callers that ask together share one build. */
  const buildLibrary = (device: string): Promise<void> => {
    let run = builds.get(device);
    if (!run) {
      run = host.library
        .build(device)
        .then((report) => logBuild(device, report))
        .finally(() => builds.delete(device));
      builds.set(device, run);
    }
    return run;
  };

  const env: TransferEnv = {
    libraryDevices: ['dt2', 'dn2'],
    library: {
      scan: async (device) => {
        await buildLibrary(device);
        return host.library.scan(device);
      },
      read: async (device, path) => new Uint8Array(await host.library.read(device, path)),
    },
    moduleFiles: (id) => MODULE_FILES[id],
    patchkitWasm: () => host.firmware.patchkitWasm(),
    deviceName: async (device) => (await samples().deviceStorage(device)).name,
    // the Emulator target has its own bulk link (the daemon's usb.bulk.in/out), so the pill and the transfers there follow the same path as a device
    bulk: {
      candidates: () =>
        app().target.target().kind === 'emulator'
          ? emulatorBulk.candidates()
          : TauriBulkLink.candidates(host.ports.bulk),
      open: (id) => (id === 'emulator' ? emulatorBulk.open(id) : TauriBulkLink.open(host.ports.bulk, id)),
      // the OUT-only interface of a real device (USB AUDIO/MIDI mode); the Emulator target has none here
      outOnlyCandidates: () =>
        app().target.target().kind === 'emulator'
          ? Promise.resolve([])
          : TauriBulkLink.outOnlyCandidates(host.ports.bulk),
      openOutOnly: (id, poll) => TauriBulkLink.openOutOnly(host.ports.bulk, id, poll),
    },
    targets: {
      emulator: true,
      inHeader: true,
      ports: () => host.ports.midi.ports(),
      async open(t: Target) {
        if (t.kind === 'emulator') return new EmuUsbTransport(host.rpc);
        if (t.kind === 'none') throw new Error('No MIDI port.');
        await host.ports.midi.open(t.port);
        return new MidiPortTransport(host.ports.midi);
      },
      async close(t: Target) {
        if (t.kind === 'midi') await host.ports.midi.close();
      },
      async preflight(t: Target) {
        if (t.kind === 'emulator' && (await emulatorMode(host.rpc)) === 'paused') throw new Error(PAUSED);
      },
    },
    logRun: (run) => logInput(transferEntry(run)),
    extras: {
      samples: samplesSlot(app, samples),
      reloadWithRuntime,
      onJobEnd() {
        // the Card panel reads the folder now and again after the emulator's quiet period (the folder follows the card about 2 s later)
        bumpCardRefresh();
        window.setTimeout(bumpCardRefresh, 2500);
      },
    },
  };
  return env;
}

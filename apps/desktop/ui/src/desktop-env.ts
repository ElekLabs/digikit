// The desktop workbench's side of the Transfer pane (packages/workbench-core/src/transfer/env.ts): the library folder of built modules, the MIDI ports and
// the emulator through the Host (host/host.ts), the samples mode and the card-folder strip of the Files page. main.tsx builds it with `createDesktopApps`.
import { EmuBulkLink } from '@digi/dk-session';
import { deviceNameOf, runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import type { SamplesSlot, TransferEnv } from '@digi/workbench-core/transfer/env.ts';
import type { Target } from '@digi/workbench-core/transfer/target.ts';
import { bumpCardRefresh } from './card-state';
import { hostWasmSource } from './desktop-wasm';
import { TauriBulkLink } from '@digi/workbench-core/host/bulk-link.ts';
import { EmuUsbTransport, emulatorMode } from '@digi/workbench-core/host/emu-transport.ts';
import type { LogWriter } from './log-state';
import { transferEntry } from './log-core';
import type { Host, LibBuildReport } from '@digi/workbench-core/host/host.ts';
import { MidiPortTransport } from '@digi/workbench-core/host/midi-transport.ts';
export const PAUSED =
  'The emulator is paused: nothing would run the transfer. Resume it (the Run button of the workbench), then try again.';

/** One entry per module that did not build, one for what changed; nothing when all is unchanged. */
function logBuild(device: string, report: LibBuildReport, logInput: LogWriter['logInput']): void {
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
 * The Transfer environment of the desktop app over `host`. `app` is the app that will own it (the bulk link uses its
 * target and page state); it is a function because the app is built from the environment.
 */
export function createDesktopEnv(
  host: Host,
  app: () => TransferApp,
  writer: LogWriter,
  samples: SamplesSlot,
  reloadWithRuntime: () => void,
): TransferEnv {
  const emulatorBulk = EmuBulkLink.opener((method, params) => host.rpc.call(method, params));
  const builds = new Map<string, Promise<void>>();
  /** Build and pack the device's run-time modules (`lib_build`) before the library is read; callers that ask together share one build. */
  const buildLibrary = (device: string): Promise<void> => {
    let run = builds.get(device);
    if (!run) {
      run = host.library
        .build(device)
        .then((report) => logBuild(device, report, writer.logInput))
        .finally(() => builds.delete(device));
      builds.set(device, run);
    }
    return run;
  };

  const env: TransferEnv = {
    libraryDevices: [...runtimeDeviceIds],
    library: {
      scan: async (device) => {
        await buildLibrary(device);
        return host.library.scan(device);
      },
      read: async (device, path) => new Uint8Array(await host.library.read(device, path)),
    },
    wasm: hostWasmSource(host),
    deviceName: async (device) => deviceNameOf(device),
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
        return MidiPortTransport.open(host.ports.midi, t.port);
      },
      async preflight(t: Target) {
        if (t.kind === 'emulator' && (await emulatorMode(host.rpc)) === 'paused') throw new Error(PAUSED);
      },
    },
    logRun: (run) => writer.logInput(transferEntry(run)),
    extras: {
      samples,
      reloadWithRuntime,
      onJobEnd() {
        // the card-folder strip reads the folder now and again after the emulator's quiet period (the folder follows the card about 2 s later)
        bumpCardRefresh();
        window.setTimeout(bumpCardRefresh, 2500);
      },
    },
  };
  return env;
}

import { createPreviewSymbols } from './symbols-mock';
// Mount the production workbench with the preview's device, filesystem and audio fixtures.
import { demoStore } from '@digi/workbench-core/target-ops/fake-store.ts';
import { createDesktopApps } from '../src/desktop-app';
import { createTauriHost } from '../src/host/tauri-host';
import { mountWorkbench } from '../src/mount-workbench';
import { setLogOpen } from '../src/log-state';
import { setLocalFolder } from '../src/samples-state';
import { createPreviewAnalyzer } from './analyzer-mock';
import { previewFirmware } from './default-modules-mock';
import { demoLibrary } from './files-mock';
import { previewSessions } from './remote-mock';
import { previewLog } from './runtime-log-mock';
import { previewNames } from './runtime-names-mock';
import { SAMPLES_ROOT, bindSimApp, control, sim } from './sim';

const query = new URLSearchParams(location.search);
const page = query.get('page') ?? 'transfer';
const host = { ...createTauriHost(), analyzer: createPreviewAnalyzer(), symbols: createPreviewSymbols() };
const apps = createDesktopApps(host, undefined, page === 'firmware' ? undefined : previewSessions());
bindSimApp(apps.app);
const files = demoStore();
if (page === 'remote' || page === 'transfer' || (page === 'files' && !query.has('card'))) {
  apps.app.target.startOn({ kind: 'midi', port: page === 'remote' ? 'Elektron Digitone II' : 'Elektron Digitakt II' });
} else if (page === 'emulator' || query.has('card')) {
  apps.app.target.startOn({ kind: 'emulator' });
}
apps.app.state.setMode(sim.mode);
setLocalFolder(sim.folder === 'none' ? '' : SAMPLES_ROOT);
(window as unknown as { __sim: typeof control }).__sim = control;
if (page === 'log') setLogOpen(true);

mountWorkbench(
  host,
  { app: apps.app, firmware: previewFirmware(apps) },
  {
    initialPage: page === 'transfer' || page === 'files' || page === 'firmware' || page === 'settings' ? page : 'run',
    files: { store: files, library: () => demoLibrary(files) },
    ...(page === 'log'
      ? {
          initialDockHeight: Number(query.get('logh') ?? 360),
          logSources: {
            emulator: {
              running: previewLog.emulatorRunning,
              session: async () => previewLog.emulator,
              makeNames: () => previewNames,
            },
            device: { port: previewLog.devicePort, session: async () => previewLog.device, present: async () => true },
          },
        }
      : {}),
  },
);

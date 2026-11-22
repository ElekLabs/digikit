import { firmwarePort } from './firmware-mock';
import { emulator } from './tauri-mock';
import { createPreviewSymbols } from './symbols-mock';
import { createDesktopApps } from '../src/desktop-app';
import { createTauriHost } from '../src/host/tauri-host';
import type { WorkbenchEnvironment } from '../src/workbench-environment';
import { setLogOpen } from '../src/log-state';
import { setLocalFolder } from '../src/samples-state';
import { createPreviewAnalyzer } from './analyzer-mock';
import { previewFirmware } from './default-modules-mock';
import { createPreviewLibrary } from './module-library';
import { previewSessions } from './remote-mock';
import { previewLog } from './runtime-log-mock';
import { createPreviewNames } from './runtime-names-mock';
import { PARAMS, SAMPLES_ROOT, bindSimApp, control, sim } from './sim';

export function createWorkbenchEnvironment(): WorkbenchEnvironment {
  const query = new URLSearchParams(location.search);
  const transferFixture = PARAMS.some((name) => query.has(name)) || query.has('lib');
  const page = query.get('page') ?? (transferFixture ? 'transfer' : undefined);
  const fixture = createPreviewLibrary();
  const host = {
    ...createTauriHost(),
    library: fixture.library,
    analyzer: createPreviewAnalyzer(),
    symbols: createPreviewSymbols(),
  };
  const apps = createDesktopApps(host, undefined, previewSessions(fixture, emulator.hasRuntime));
  bindSimApp(apps.app);
  if (page === 'firmware') {
    apps.app.target.startOn({ kind: 'midi', port: firmwarePort });
  } else if (page === 'remote' || page === 'transfer' || (page === 'files' && !query.has('card'))) {
    apps.app.target.startOn({
      kind: 'midi',
      port: page === 'remote' ? 'Elektron Digitone II' : 'Elektron Digitakt II',
    });
  } else if (page === 'emulator' || query.has('card')) {
    apps.app.target.startOn({ kind: 'emulator' });
  }
  if (query.has('mode')) apps.app.state.setMode(sim.mode);
  setLocalFolder(sim.folder === 'none' ? '' : SAMPLES_ROOT);
  (window as unknown as { __sim: typeof control }).__sim = control;
  if (page === 'log') setLogOpen(true);

  return {
    host,
    apps: query.has('defmods') ? { ...apps, firmware: previewFirmware(apps) } : apps,
    options: {
      browserPreview: true,
      initialPage:
        page === 'transfer' || page === 'files' || page === 'firmware' || page === 'settings' ? page : undefined,
      ...(page === 'log'
        ? {
            initialDockHeight: Number(query.get('logh') ?? 360),
            logSources: {
              emulator: {
                running: previewLog.emulatorRunning,
                session: async () => previewLog.emulator,
                makeNames: () => createPreviewNames(host.firmware),
              },
              device: {
                port: previewLog.devicePort,
                session: async () => previewLog.device,
                present: async () => true,
              },
            },
          }
        : {}),
    },
  };
}

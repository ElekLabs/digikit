import { createPreviewStores } from './module-store';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import { createPreviewModulePages } from './module-pages';
import { firmwarePort } from './firmware-mock';
import { emulator } from './tauri-mock';
import { createPreviewSymbols } from './symbols-mock';
import { createDesktopApps } from '../src/desktop-app';
import { createTauriHost } from '../src/host/tauri-host';
import type { WorkbenchEnvironment } from '../src/workbench-environment';
import { localStorageKeyValue } from '@digi/workbench-core/storage.ts';
import { SAMPLE_FOLDER_KEY } from '../src/samples/source';
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
  const modulePages = createPreviewModulePages();
  const fixture = createPreviewLibrary();
  const stores = createPreviewStores(fixture, modulePages);
  const base = createTauriHost();
  const rpc: EmuRpc = {
    async call<T>(method: string, params?: Record<string, unknown>, session?: number) {
      const result = await base.rpc.call<T>(method, params, session);
      if (method === 'firmware.load' || method === 'restart' || method === 'dev.reload') {
        await stores.restart('emulator');
      }
      return result;
    },
  };
  const host = {
    ...base,
    rpc,
    library: fixture.library,
    analyzer: createPreviewAnalyzer(),
    symbols: createPreviewSymbols(),
  };
  const storage = localStorageKeyValue();
  if (sim.folder === 'none') storage.remove(SAMPLE_FOLDER_KEY);
  else storage.set(SAMPLE_FOLDER_KEY, SAMPLES_ROOT);
  const apps = createDesktopApps(host, storage, previewSessions(stores, emulator.hasRuntime));
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
  (window as unknown as { __sim: typeof control }).__sim = control;
  if (page === 'log') apps.drawer.select('Log');

  return {
    host,
    apps: query.has('defmods') ? { ...apps, firmware: previewFirmware(apps) } : apps,
    options: {
      browserPreview: true,
      modulePages,
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

// The desktop workbench's apps: the Transfer app (target, connection, jobs, page state) and the Firmware app on it, built once at startup from
// the Host (host/host.ts) and the webview's storage. main.tsx gives them to the pages (packages/workbench-core/ui/transfer/AppProvider.tsx).
import { DkSession } from '@digi/dk-session';
import { createFirmwareApp, type FirmwareApp } from '@digi/workbench-core/firmware/app.ts';
import { localStorageKeyValue, type KeyValue } from '@digi/workbench-core/storage.ts';
import { createTransferApp, type TransferApp } from '@digi/workbench-core/transfer/app.ts';
import { runTransfer } from '@digi/workbench-core/transfer/run-transfer.ts';
import { createSamplesApp } from './samples/app';
import { createSamplesSlot } from './samples/slot';
import { createSessionHub } from '@digi/workbench-core/transfer/session-core.ts';
import { createEmulatorActions } from './emulator-actions';
import { createDesktopEnv } from './desktop-env';
import { createDesktopFirmwareEnv } from './desktop-firmware';
import { createLogDrawer, type DrawerSelection, type LogReader, type LogWriter } from './log-state';
import type { Host } from '@digi/workbench-core/host/host.ts';

export interface DesktopApps {
  app: TransferApp;
  firmware: FirmwareApp;
  drawer: DrawerSelection;
  log: LogReader;
  logWriter: LogWriter;
  emulatorActions: ReturnType<typeof createEmulatorActions>;
}

export function createDesktopApps(
  host: Host,
  storage: KeyValue = localStorageKeyValue(),
  sessions = createSessionHub((transport, opts) => new DkSession(transport, { timeoutMs: 6000, ...opts })),
): DesktopApps {
  const tools = createLogDrawer(storage);
  const emulatorActions = createEmulatorActions();
  let app!: TransferApp;
  const getApp = () => app;
  const samples = createSamplesApp({
    fs: host.fs,
    firmware: host.firmware,
    storage,
    getTarget: () => getApp().target,
    getState: () => getApp().state,
    getConnection: () => getApp().connection,
    getBulk: () => getApp().env.bulk,
    run: (what, job) => runTransfer(getApp(), what, job),
    writer: tools.logWriter,
  });
  app = createTransferApp(
    createDesktopEnv(host, getApp, tools.logWriter, createSamplesSlot(samples), emulatorActions.reloadWithRuntime),
    { storage, sessions },
  );
  return {
    ...tools,
    emulatorActions,
    app,
    firmware: createFirmwareApp(createDesktopFirmwareEnv(host, getApp, tools.logWriter), app),
  };
}

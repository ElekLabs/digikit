// The desktop workbench's apps: the Transfer app (target, connection, jobs, page state) and the Firmware app on it, built once at startup from
// the Host (host/host.ts) and the webview's storage. main.tsx gives them to the pages (apps/web/src/transfer/AppProvider.tsx).
import { DkSession } from '@digi/dk-session';
import { createFirmwareApp, type FirmwareApp } from '../../../web/src/firmware/app';
import { localStorageKeyValue, type KeyValue } from '../../../web/src/storage';
import { createTransferApp, type TransferApp } from '../../../web/src/transfer/app';
import { createSessionHub } from '../../../web/src/transfer/session-core';
import { createDesktopEnv } from './desktop-env';
import { createDesktopFirmwareEnv } from './desktop-firmware';
import type { Host } from './host/host';

export interface DesktopApps {
  app: TransferApp;
  firmware: FirmwareApp;
}

export function createDesktopApps(host: Host, storage: KeyValue = localStorageKeyValue()): DesktopApps {
  let app!: TransferApp;
  const getApp = () => app;
  app = createTransferApp(createDesktopEnv(host, getApp), {
    storage,
    sessions: createSessionHub((transport) => new DkSession(transport, { timeoutMs: 6000 })),
  });
  return { app, firmware: createFirmwareApp(createDesktopFirmwareEnv(host, getApp), app) };
}

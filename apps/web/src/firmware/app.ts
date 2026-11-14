// The Firmware app: the page's state over the app's FirmwareEnv, on the app's Transfer app (the MIDI port, the jobs and the connection are the
// Transfer pane's). Built once at startup, like `createTransferApp`.
import type { TransferApp } from '../transfer/app';
import { createDefaultModules, type DefaultModulesApp } from './default-modules-state';
import type { FirmwareEnv } from './env';
import { createFirmwareState, type FirmwareState } from './firmware-state';

export interface FirmwareApp {
  env: FirmwareEnv;
  transfer: TransferApp;
  defaults: DefaultModulesApp;
  state: FirmwareState;
}

export function createFirmwareApp(env: FirmwareEnv, transfer: TransferApp): FirmwareApp {
  const defaults = createDefaultModules(env, transfer.storage);
  const state = createFirmwareState({ env, transfer, defaults });
  return { env, transfer, defaults, state };
}

import { createDeviceModuleChannel } from './device-module-channel.ts';
import { deviceQueryChannel } from './device-queries.ts';
import { deviceSettingsChannel } from './device-settings.ts';
import type { ModuleChannelFactory } from './module-channel-factory.ts';
import { bindModuleFiles } from './module-files.ts';
import type { SaveFile, SaveResult } from '@digi/module-ui';
import type { TransferApp } from '../transfer/app.ts';

/** The factory of a real target: settings over the module's session endpoint, files over its data directory. */
export function createDeviceChannelFactory(
  app: Pick<TransferApp, 'target'>,
  /** The host's save of files into a folder the user picks; the app object passes it in. */
  saveFiles?: (files: SaveFile[]) => Promise<SaveResult>,
): ModuleChannelFactory {
  return ({ module, current }) => {
    const binding = app.target.bindSession();
    const { index, uiInstance } = module;
    return createDeviceModuleChannel({
      settings: deviceSettingsChannel(
        module.name,
        uiInstance ? async () => (await binding.session()).moduleSettings(index, uiInstance, current) : undefined,
      ),
      queries: deviceQueryChannel(
        uiInstance ? async () => (await binding.session()).moduleQueries(index, uiInstance, current) : undefined,
      ),
      files: bindModuleFiles(module.name, binding, current),
      saveFiles,
    });
  };
}

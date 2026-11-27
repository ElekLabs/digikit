// The ModuleChannel of one loaded module instance: its settings and query requests (the device's, or a memory channel in the preview) and files over
// the module's data directory.
import {
  checkSaveFiles,
  type ModuleChannel,
  type ModuleChannelEvent,
  type ModuleFileClient,
  type SaveFile,
  type SaveResult,
} from '@digi/module-ui';
import type { ModuleQueryChannel } from './device-queries.ts';
import type { ModuleSettingsChannel } from './device-settings.ts';

export interface ModuleChannelParts {
  settings: ModuleSettingsChannel;
  queries: ModuleQueryChannel;
  files: ModuleFileClient;
  /** The host's save of files into a folder the user picks (`FsHost.saveFiles`); an app without one refuses. */
  saveFiles?: (files: SaveFile[]) => Promise<SaveResult>;
}

export function createDeviceModuleChannel({ settings, queries, files, saveFiles }: ModuleChannelParts): ModuleChannel {
  const listeners = new Set<(event: ModuleChannelEvent) => void>();
  return {
    describeSettings: () => settings.describeSettings(),
    getSetting: (id) => settings.getSetting(id),
    putSetting: (id, bytes, revision) => settings.putSetting(id, bytes, revision),
    resetSetting: (id, revision) => settings.resetSetting(id, revision),
    callQuery: (method, request) => queries.callQuery(method, request),
    listFiles: () => files.list(),
    readFile: (name) => files.read(name),
    sendFile: (job, name, bytes) =>
      files.send(name, bytes, (done, total) => {
        for (const listener of listeners) listener({ kind: 'file-progress', job, done, total });
      }),
    removeFile: (name) => files.remove(name),
    async saveFiles(list) {
      checkSaveFiles(list);
      if (!saveFiles) throw new Error('This app cannot save files.');
      return saveFiles(list);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}

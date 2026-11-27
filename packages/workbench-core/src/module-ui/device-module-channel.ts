// The ModuleChannel of one loaded module instance: its settings and query requests (the device's, or a memory channel in the preview) and files over
// the module's data directory.
import type { ModuleChannel, ModuleChannelEvent, ModuleFileClient } from '@digi/module-ui';
import type { ModuleQueryChannel } from './device-queries.ts';
import type { ModuleSettingsChannel } from './device-settings.ts';

export interface ModuleChannelParts {
  settings: ModuleSettingsChannel;
  queries: ModuleQueryChannel;
  files: ModuleFileClient;
}

export function createDeviceModuleChannel({ settings, queries, files }: ModuleChannelParts): ModuleChannel {
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
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}

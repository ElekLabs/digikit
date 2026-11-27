// The real ModuleChannel of one loaded module instance: settings over its session endpoint, files over the module's data directory.
import type { ModuleSettingsEndpoint } from '@digi/dk-session';
import type { DescribedSchema, ModuleChannel, ModuleChannelEvent, ModuleFileClient, RawSetting } from '@digi/module-ui';
import { settingError } from '@digi/module-ui';
import { deviceSettingRequests } from './device-settings.ts';

export interface DeviceModuleParts {
  schema: DescribedSchema;
  /** Absent while the module has no settings endpoint: every settings request then fails with `unavailable`. */
  endpoint?: ModuleSettingsEndpoint;
  files: ModuleFileClient;
}

export function createDeviceModuleChannel({ schema, endpoint, files }: DeviceModuleParts): ModuleChannel {
  const listeners = new Set<(event: ModuleChannelEvent) => void>();
  const none = async (): Promise<RawSetting> => ({
    ok: false,
    error: settingError('unavailable', 'The module has no public settings.'),
  });
  const settings = endpoint
    ? deviceSettingRequests(schema, endpoint)
    : { getSetting: none, putSetting: none, resetSetting: none };
  return {
    ...settings,
    describeSettings: async () => schema,
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

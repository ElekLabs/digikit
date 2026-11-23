import type { ModuleSettingsEndpoint, PublicModuleSettings } from '@digi/dk-session';
import type { SettingsClient } from './module-page';

export function publicSettingsClient(schema: PublicModuleSettings, endpoint: ModuleSettingsEndpoint): SettingsClient {
  function index(key: string) {
    const index = schema.settings.findIndex((field) => field.id === key);
    if (index < 0) throw new Error('Unknown public setting.');
    return index;
  }
  return {
    async get(key) {
      return endpoint.get(index(key));
    },
    async put(key, value, revision) {
      const i = index(key);
      const field = schema.settings[i]!;
      if (new TextEncoder().encode(value).length > field.maxBytes)
        throw new Error(`${field.label} allows up to ${field.maxBytes} bytes.`);
      return endpoint.put(i, value, revision);
    },
    async reset(key, revision) {
      return endpoint.reset(index(key), revision);
    },
  };
}

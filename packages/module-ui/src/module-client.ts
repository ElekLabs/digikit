import type { ModuleChannel } from './module-channel.ts';
import { ChannelFiles, type ModuleFileClient } from './module-file-client.ts';
import type { SettingsOf, SettingsSchema } from './settings-schema.ts';
import { ModuleSettings, type TypedSettings } from './typed-settings.ts';

/**
 * What a module page uses: typed settings and the module's own files. It is a page-side helper over a `ModuleChannel`, so the same page
 * runs compiled into the app or inside a sandboxed frame. Operations come in phase 2.
 */
export interface ModuleClient {
  settings<S extends SettingsSchema>(schema: S): TypedSettings<SettingsOf<S>>;
  readonly files: ModuleFileClient;
}

export function createModuleClient(channel: ModuleChannel): ModuleClient {
  return { settings: (schema) => new ModuleSettings(schema, channel), files: new ChannelFiles(channel) };
}

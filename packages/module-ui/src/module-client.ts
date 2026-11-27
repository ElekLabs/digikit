import type { ModuleFileClient } from './module-file-client.ts';
import type { SettingsSchema } from './settings-schema.ts';
import type { TypedSettings } from './typed-settings.ts';
import type { SettingsOf } from './settings-schema.ts';

/**
 * What a module page may reach: the module's typed settings and its own files. Every call is async and takes and returns structured-clone
 * data, so the same page runs compiled into the app or inside a sandboxed frame. Operations come in phase 2.
 */
export interface ModuleClient {
  settings<S extends SettingsSchema>(schema: S): TypedSettings<SettingsOf<S>>;
  readonly files: ModuleFileClient;
}

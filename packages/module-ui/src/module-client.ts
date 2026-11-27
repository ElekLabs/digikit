import type { ModuleChannel } from './module-channel.ts';
import { ChannelFiles, type ModuleFileClient } from './module-file-client.ts';
import type { DescService } from '@bufbuild/protobuf';
import { createServiceClient, type ServiceClient } from './query-service-client.ts';
import type { SettingsOf, SettingsSchema } from './settings-schema.ts';
import { ModuleSettings, type TypedSettings } from './typed-settings.ts';

/**
 * What a module page uses: typed settings, the module's own files and typed queries. It is a page-side helper over a `ModuleChannel`, so the same page
 * runs compiled into the app or inside a sandboxed frame.
 */
export interface ModuleClient {
  settings<S extends SettingsSchema>(schema: S): TypedSettings<SettingsOf<S>>;
  /**
   * One async function per method of the module's protobuf-es service; results are `{ ok, value }` or `{ ok: false, error }`. After a `timeout`
   * the device may still hold the call: a retry can answer `busy` for up to about 2 s, then works.
   */
  service<D extends DescService>(desc: D): ServiceClient<D>;
  readonly files: ModuleFileClient;
}

export function createModuleClient(channel: ModuleChannel): ModuleClient {
  return {
    settings: (schema) => new ModuleSettings(schema, channel),
    service: (desc) => createServiceClient(channel, desc),
    files: new ChannelFiles(channel),
  };
}

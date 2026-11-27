import {
  MemorySettingsStore,
  createMemoryModuleChannel,
  settingError,
  type DescribedSchema,
  type ModuleFileClient,
  type RawSetting,
} from '@digi/module-ui';
import { QUERY_REPLY_MAX, QUERY_REQUEST_MAX } from '@digi/dk-session';
import { createDeviceModuleChannel } from '@digi/workbench-core/module-ui/device-module-channel.ts';
import type {
  ModuleChannelFactory,
  ModuleChannelRequest,
} from '@digi/workbench-core/module-ui/module-channel-factory.ts';
import { previewQueryServices, type PreviewQueryFactory } from './module-query-services';

/**
 * The channels of the simulated targets. Settings are a memory channel of the module's generated schema: one store per target and module,
 * kept across reloads of the instance, while each instance gets a fresh channel that fails once the instance or the target has gone. Files
 * are the simulated card's, through the same binding as a real target; like the runtime, a file-name setting takes only a file the card holds.
 */
export function createPreviewChannels(
  schemas: Map<string, DescribedSchema>,
  /** The files of a module on the simulated card, bound to the instance's lifetime. */
  filesOf: (request: ModuleChannelRequest) => ModuleFileClient,
  /** The TypeScript handlers of a module's queries, by module id; the device limits of a request and a reply apply to them. */
  queryServices: Map<string, PreviewQueryFactory> = previewQueryServices(),
): ModuleChannelFactory {
  const stores = new Map<string, MemorySettingsStore>();
  return (request) => {
    const { target, module, current } = request;
    const schema = schemas.get(module.name);
    if (!schema) throw new Error(`The preview has no settings for ${module.name}.`);
    const key = `${target}\n${module.name}`;
    let store = stores.get(key);
    if (!store) {
      store = new MemorySettingsStore(schema);
      stores.set(key, store);
    }
    const files = filesOf(request);
    const loaded = (id: string) => {
      const read = store.read(id);
      return read.ok ? read.value : undefined;
    };
    const service = queryServices.get(module.name)?.(loaded);
    const memory = createMemoryModuleChannel(schema, {
      store,
      current,
      service,
      queryLimits: { request: QUERY_REQUEST_MAX, reply: QUERY_REPLY_MAX },
    }).channel;
    return createDeviceModuleChannel({
      settings: {
        ...memory,
        async putSetting(id, bytes, revision): Promise<RawSetting> {
          const setting = schema.settings.find((s) => s.id === id);
          if (setting?.kind === 'file-name') {
            const name = new TextDecoder().decode(bytes);
            const held = (await files.list().catch(() => [])).some((file) => file.name === name && file.size > 0);
            if (!name.toLowerCase().endsWith(`.${setting.extension}`) || !held)
              return { ok: false, error: settingError('invalid', 'The selected file is missing.') };
          }
          return memory.putSetting(id, bytes, revision);
        },
      },
      queries: memory,
      files,
    });
  };
}

import { parse } from 'smol-toml';
import type { DkModule, ModuleSettingsEndpoint, RawSettingValue } from '@digi/dk-session';
import {
  encodeDescriptor,
  MemorySettingsStore,
  type DescribedSchema,
  type SettingError,
  type SettingsSchema,
} from '@digi/module-ui';
import { moduleFilePath } from '@digi/workbench-core/modules/dktransfer.ts';

const manifests = import.meta.glob<string>('../../../../modules/*/*/*/module.toml', {
  eager: true,
  import: 'default',
  query: '?raw',
});
// The settings each module declares in workbench.toml, as patchkit generated them.
const generated = import.meta.glob<{ schema: SettingsSchema }>('../../../../modules/*/*/*/workbench/gen/settings.ts', {
  eager: true,
});

export function previewSettingsSchemas() {
  const schemas = new Map<string, DescribedSchema>();
  for (const [path, { schema }] of Object.entries(generated)) {
    const manifest = manifests[path.replace(/workbench\/gen\/settings\.ts$/, 'module.toml')];
    if (!manifest) continue;
    const { module } = parse(manifest) as { module: { id: string; name: string } };
    schemas.set(module.id, { ...schema, name: module.name });
  }
  return schemas;
}

function raw(result: ReturnType<MemorySettingsStore['read']>): RawSettingValue {
  if (!result.ok) throw refusal(result.error);
  return { value: result.value, revision: result.revision };
}

function refusal(error: SettingError) {
  const status = { conflict: 'E_MODULE_UI_CONFLICT', 'overlay-scope': 'E_MODULE_UI_SCOPE' } as Record<string, string>;
  return Object.assign(new Error(error.message), { kind: 'device', status: status[error.code] });
}

export function createPreviewSettings(
  schemas: Map<string, DescribedSchema>,
  store: () => Promise<{
    files: Map<string, Uint8Array>;
    boot: { modules: DkModule[] };
    settings: Map<string, MemorySettingsStore>;
  }>,
  slot: number,
  instance: number,
  current: () => boolean,
): ModuleSettingsEndpoint {
  async function binding() {
    const card = await store();
    const loaded = card.boot.modules.find((module) => module.index === slot && module.uiInstance === instance);
    const schema = loaded && schemas.get(loaded.name);
    if (!current() || !loaded || !schema) throw new Error('The module page has closed.');
    let values = card.settings.get(loaded.name);
    if (!values) {
      values = new MemorySettingsStore(schema);
      card.settings.set(loaded.name, values);
    }
    return { card, schema, values, id: loaded.name };
  }
  const field = (schema: DescribedSchema, index: number) => {
    const match = schema.settings[index];
    if (!match) throw new Error('Unknown public setting.');
    return match;
  };
  return {
    async describe() {
      const { schema } = await binding();
      return encodeDescriptor(schema.name, schema);
    },
    async get(index) {
      const { schema, values } = await binding();
      return raw(values.read(field(schema, index).id));
    },
    async put(index, value, revision) {
      const { card, schema, values, id } = await binding();
      const setting = field(schema, index);
      if (setting.kind === 'file-name') {
        const name = new TextDecoder().decode(value);
        if (!name.toLowerCase().endsWith(`.${setting.extension}`) || !card.files.get(moduleFilePath(id, name))?.length)
          throw new Error('The selected file is missing.');
      }
      return raw(values.change(setting.id, value, revision));
    },
    async reset(index, revision) {
      const { schema, values } = await binding();
      return raw(values.change(field(schema, index).id, undefined, revision));
    },
  };
}

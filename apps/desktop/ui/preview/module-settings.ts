import { parse } from 'smol-toml';
import { DkError, type DkModule, type RawSettingValue } from '@digi/dk-session';
import { MemorySettingsStore, type DescribedSchema, type SettingError, type SettingsSchema } from '@digi/module-ui';
import type { SettingsEndpoint } from '@digi/workbench-core/module-ui/device-settings.ts';
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
  if (!result.ok) throw previewRefusal(result.error);
  return { value: result.value, revision: result.revision };
}

/** The error the device sends for a refused setting request: the same `DkError` the session throws, so the real adapter maps it. */
export function previewRefusal(error: SettingError): Error {
  const status: Partial<Record<SettingError['code'], string>> = {
    conflict: 'E_MODULE_UI_CONFLICT',
    'overlay-scope': 'E_MODULE_UI_SCOPE',
    invalid: 'E_SET_VALUE',
    busy: 'E_AGAIN',
  };
  const named = status[error.code];
  return named ? new DkError({ error: error.message, kind: 'device', status: named }) : new Error(error.message);
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
): SettingsEndpoint {
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
    // The simulated device holds its generated schema; a real one reports a descriptor (see `describeDeviceSettings`).
    describeSchema: async () => (await binding()).schema,
    describe: async () => {
      throw new Error('The preview reports its schema directly.');
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

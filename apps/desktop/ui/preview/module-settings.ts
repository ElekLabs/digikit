import { parse } from 'smol-toml';
import type { DkModule, ModuleSettingsEndpoint, PublicField, PublicModuleSettings } from '@digi/dk-session';
import { createMemorySettings } from '@digi/workbench-core/module-ui/memory-client.ts';
import { moduleFilePath } from '@digi/workbench-core/modules/dktransfer.ts';

const manifests = import.meta.glob<string>('../../../../modules/*/*/*/module.toml', {
  eager: true,
  import: 'default',
  query: '?raw',
});
const declarations = import.meta.glob<string>('../../../../modules/*/*/*/workbench.toml', {
  eager: true,
  import: 'default',
  query: '?raw',
});

export function previewSettingsSchemas() {
  const schemas = new Map<string, PublicModuleSettings>();
  for (const [path, text] of Object.entries(declarations)) {
    const manifest = manifests[path.replace(/workbench\.toml$/, 'module.toml')];
    if (!manifest) continue;
    const { module } = parse(manifest) as { module: { id: string; name: string } };
    const { setting } = parse(text) as {
      setting?: Array<{
        id: string;
        label: string;
        type: PublicField['kind'];
        extension?: string;
        reset_label?: string;
        apply?: PublicField['apply'];
        max_bytes: number;
      }>;
    };
    const settings = (setting ?? []).map(
      ({ id, label, apply = 'immediate', max_bytes: maxBytes, type, extension, reset_label }): PublicField =>
        type === 'file-name'
          ? { id, label, apply, maxBytes, kind: type, extension: extension!, resetLabel: reset_label ?? 'None' }
          : { id, label, apply, maxBytes, kind: 'text' },
    );
    schemas.set(module.id, { name: module.name, settings });
  }
  return schemas;
}

export function createPreviewSettings(
  schemas: Map<string, PublicModuleSettings>,
  store: () => Promise<{
    files: Map<string, Uint8Array>;
    boot: { modules: DkModule[] };
    settings: Map<string, ReturnType<typeof createMemorySettings>>;
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
      values = createMemorySettings();
      card.settings.set(loaded.name, values);
    }
    return { card, schema, values, id: loaded.name };
  }
  const field = (schema: PublicModuleSettings, index: number) => {
    const match = schema.settings[index];
    if (!match) throw new Error('Unknown public setting.');
    return match;
  };
  return {
    async describe() {
      return (await binding()).schema;
    },
    async get(index) {
      const { schema, values } = await binding();
      return values.get(field(schema, index).id);
    },
    async put(index, value, revision) {
      const { card, schema, values, id } = await binding();
      const setting = field(schema, index);
      if (new TextEncoder().encode(value).length > setting.maxBytes) throw new Error('Setting value is too long.');
      if (
        setting.kind === 'file-name' &&
        (!value.toLowerCase().endsWith(`.${setting.extension}`) || !card.files.get(moduleFilePath(id, value))?.length)
      )
        throw new Error('The selected file is missing.');
      return values.change(setting.id, value, revision);
    },
    async reset(index, revision) {
      const { schema, values } = await binding();
      return values.change(field(schema, index).id, undefined, revision);
    },
  };
}

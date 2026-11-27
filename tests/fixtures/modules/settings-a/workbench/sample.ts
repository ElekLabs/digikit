import { createMemoryModuleChannel, createModuleClient, MemorySettingsStore, settingValueCodec, type SettingsSchema } from '@digi/module-ui';
import type { PageDescriptor } from '@digi/workbench-core/module-ui/module-page.ts';
import { schema } from './gen/settings';
import manifest from '../module.toml?raw';

const name = manifest.match(/^name = "(.*)"$/m)?.[1];
if (!name) throw new Error('Expected a module name in module.toml.');

export const moduleId = schema.module;
// The Panel is the companion in this folder; the generated page registry will replace this link.
export const descriptor: PageDescriptor = { id: moduleId, name, component: 'workbench/Panel.tsx', schema };

export function createSettingsSample() {
  const store = new MemorySettingsStore(schema);
  let generation = 0;
  return {
    boot(active: boolean) {
      const instance = ++generation;
      if (!active) return undefined;
      const stored = store.read('boot');
      const snapshot = stored.ok && stored.value ? settingValueCodec.decode(schema.settings[0], stored.value) : undefined;
      return (current: () => boolean) => {
        const live = () => current() && generation === instance;
        const client = createModuleClient(createMemoryModuleChannel(schema, { store, current: live }).channel);
        return {
          settings: client.settings(schema as SettingsSchema),
          async readAtInit() {
            if (!live()) throw new Error('This module page has closed.');
            return snapshot;
          },
        };
      };
    },
  };
}

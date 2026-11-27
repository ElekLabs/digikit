import { readModuleDescriptor } from '@digi/workbench-core/module-ui/descriptor.ts';
import { createMemoryModuleClient, createMemorySettings } from '@digi/workbench-core/module-ui/memory-client.ts';
import source from '../workbench.toml?raw';

import manifest from '../module.toml?raw';

export const descriptor = readModuleDescriptor(source, manifest);
export const moduleId = descriptor.id;

export function createSettingsSample() {
  const store = createMemorySettings();
  let generation = 0;
  return {
    boot(active: boolean) {
      const instance = ++generation;
      if (!active) return undefined;
      const snapshot = store.get('boot').value;
      return (current: () => boolean) => {
        const client = createMemoryModuleClient(
          descriptor,
          store,
          { readAtInit: () => snapshot },
          () => current() && generation === instance,
        );
        return { settings: client.settings, readAtInit: () => client.call('readAtInit') };
      };
    },
  };
}

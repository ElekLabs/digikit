import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import type Panel from './Panel';
import { createSettingsSample, descriptor } from './sample';

export { moduleId } from './sample';
const companions = import.meta.glob<typeof Panel>('./**/*.tsx', { eager: true, import: 'default' });

export function createPreviewSample() {
  const sample = createSettingsSample();
  let page: ModulePage | undefined;
  return {
    get page() {
      return page;
    },
    boot(active: boolean) {
      const open = sample.boot(active);
      page = open && {
        descriptor,
        open(current) {
          const client = open(current);
          const Companion = companions[descriptor.component?.replace(/^workbench\//, './') ?? ''];
          return {
            settings: client.settings,
            Panel: Companion,
          };
        },
      };
    },
  };
}

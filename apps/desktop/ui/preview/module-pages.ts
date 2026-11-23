import { createPreviewSample, moduleId } from '../../../../tests/fixtures/modules/settings-a/workbench/preview';
import { fakeDkm } from '@digi/workbench-core/target-ops/fake-store.ts';
import { modulePath } from '@digi/workbench-core/modules/dktransfer.ts';

// One persisted record per simulated target. Only a boot result creates a page.
export function createPreviewModulePages() {
  const samples = new Map<string, ReturnType<typeof createPreviewSample>>();
  return {
    seed(files: Map<string, Uint8Array>, selected: Map<string, Uint8Array | undefined>) {
      const path = modulePath(moduleId);
      files.set(path, fakeDkm({ name: moduleId }));
      selected.set(path, undefined);
    },
    boot(key: string, modules: { name: string }[]) {
      const sample = samples.get(key) ?? createPreviewSample();
      samples.set(key, sample);
      sample.boot(modules.some((module) => module.name === moduleId));
    },
    lookup(key: string, id: string) {
      return id === moduleId ? samples.get(key)?.page : undefined;
    },
  };
}
export type PreviewModulePages = ReturnType<typeof createPreviewModulePages>;

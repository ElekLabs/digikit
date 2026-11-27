import type { ModulePageLoaders } from '@digi/workbench-core/module-ui/module-page-registry.ts';
import { modulePageLoaders } from '../src/module-page-loaders';

// The pages of the modules plus the fixtures' (a fixture page ships only in the preview and in tests). A new folder joins the globs after HMR.
export const previewPageLoaders: ModulePageLoaders = {
  ...modulePageLoaders,
  ...import.meta.glob('../../../../tests/fixtures/modules/*/workbench/index.tsx'),
};

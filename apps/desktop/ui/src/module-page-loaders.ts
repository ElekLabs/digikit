import type { ModulePageLoaders } from '@digi/workbench-core/module-ui/module-page-registry.ts';

// The pages the modules of this checkout ship (`<module dir>/workbench/index.tsx`), loaded when a module is loaded on the target. A module
// folder added in the dev server joins the glob after HMR. The Nx inputs of this app list the same pattern.
export const modulePageLoaders: ModulePageLoaders = import.meta.glob('../../../../modules/*/*/*/workbench/index.tsx');

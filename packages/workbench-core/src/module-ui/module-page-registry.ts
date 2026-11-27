import { isModulePage, type ModulePageDefinition } from '@digi/module-ui';

/** Lazy loaders of the module pages by source path: what `import.meta.glob('.../workbench/index.tsx')` gives. */
export type ModulePageLoaders = Record<string, () => Promise<unknown>>;

export interface ModulePageRegistry {
  /**
   * The page a module ships, or undefined when no loader holds a page of that id at this contract version (the generic form shows then).
   * A loader that fails or that holds something else is skipped.
   */
  find(id: string): Promise<ModulePageDefinition | undefined>;
}

export function createModulePageRegistry(loaders: ModulePageLoaders): ModulePageRegistry {
  // A page's id is known only once its file has loaded, so the first lookup loads every file once.
  const pages = new Map<string, Promise<ModulePageDefinition | undefined>>();
  const load = (path: string) => {
    let page = pages.get(path);
    if (!page) {
      page = loaders[path]!().then(
        (loaded) => {
          const definition = (loaded as { default?: unknown } | undefined)?.default;
          return isModulePage(definition) ? definition : undefined;
        },
        () => {
          pages.delete(path);
          return undefined;
        },
      );
      pages.set(path, page);
    }
    return page;
  };
  return {
    async find(id) {
      for (const page of await Promise.all(Object.keys(loaders).map(load))) if (page?.id === id) return page;
      return undefined;
    },
  };
}

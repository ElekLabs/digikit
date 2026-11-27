import { isModulePage, type ModulePageDefinition } from '@digi/module-ui';

/** Lazy loaders of the module pages by source path: what `import.meta.glob('.../workbench/index.tsx')` gives. */
export type ModulePageLoaders = Record<string, () => Promise<unknown>>;

export interface ModulePageRegistry {
  /**
   * The page a module ships, or undefined when no loader holds a page of that id at this contract version (the generic form shows then).
   * A loader that fails or that holds something else is skipped (a failure is logged once per file). A page resolves as soon as its own
   * file has loaded: a slow or failing file of another module does not hold it.
   */
  find(id: string): Promise<ModulePageDefinition | undefined>;
}

export function createModulePageRegistry(loaders: ModulePageLoaders): ModulePageRegistry {
  // A page's id is known only once its file has loaded, so the first lookup loads every file once.
  const pages = new Map<string, Promise<ModulePageDefinition | undefined>>();
  const logged = new Set<string>();
  const load = (path: string) => {
    let page = pages.get(path);
    if (!page) {
      page = loaders[path]!().then(
        (loaded) => {
          const definition = (loaded as { default?: unknown } | undefined)?.default;
          return isModulePage(definition) ? definition : undefined;
        },
        (error) => {
          pages.delete(path);
          if (!logged.has(path)) {
            logged.add(path);
            console.warn(`Module page ${path} failed to load; its module shows the generic form.`, error);
          }
          return undefined;
        },
      );
      pages.set(path, page);
    }
    return page;
  };
  return {
    find(id) {
      const paths = Object.keys(loaders);
      return new Promise((resolve) => {
        let pending = paths.length;
        if (!pending) resolve(undefined);
        for (const path of paths)
          void load(path).then((page) => {
            if (page?.id === id) resolve(page);
            else if (--pending === 0) resolve(undefined);
          });
      });
    },
  };
}

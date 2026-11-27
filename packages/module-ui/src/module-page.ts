import type { Component } from 'solid-js';
import type { ModuleClient } from './module-client.ts';

/** What Workbench gives a module page: the client bound to the loaded module instance the page shows. */
export interface ModulePageProps {
  client: ModuleClient;
}

/** The version of the page contract (`ModulePageProps` and what Workbench does with `Page`). Workbench shows no page of another version. */
export const MODULE_PAGE_API = 1;

/** The default export of `<module dir>/workbench/index.tsx`. */
export interface ModulePageDefinition {
  /** The module id of module.toml (the generated `schema.module`). */
  readonly id: string;
  readonly api: typeof MODULE_PAGE_API;
  readonly Page: Component<ModulePageProps>;
}

export function definePage(page: ModulePageDefinition): ModulePageDefinition {
  return page;
}

/** True for a value that is a page of the contract version this Workbench runs: the registry's check of a loaded module. */
export function isModulePage(value: unknown): value is ModulePageDefinition {
  const page = value as Partial<ModulePageDefinition> | null;
  return (
    typeof page === 'object' &&
    page !== null &&
    page.api === MODULE_PAGE_API &&
    typeof page.id === 'string' &&
    typeof page.Page === 'function'
  );
}

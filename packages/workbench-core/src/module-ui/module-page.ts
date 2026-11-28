import type { Component } from 'solid-js';
import type { DynamicSettings, SettingsSchema } from '@digi/module-ui';

export interface PageDescriptor {
  id: string;
  name: string;
  schema: SettingsSchema;
  /** The module ships a page (`workbench/index.tsx`). */
  hasPage: boolean;
  /** The module has a file panel. */
  hasFiles: boolean;
}

// A definition belongs to one loaded instance. Opening the page binds every call
// to the lifetime of the mount and the selected connection.
export interface ModulePage {
  descriptor: PageDescriptor;
  /** `error` replaces the content: the page could not be built. */
  open(current: () => boolean): { settings?: DynamicSettings; Panel?: Component; Page?: Component; error?: string };
}

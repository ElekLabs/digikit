import type { Component } from 'solid-js';
import type { DynamicSettings, SettingsSchema } from '@digi/module-ui';

export interface PageDescriptor {
  id: string;
  name: string;
  component?: string;
  schema: SettingsSchema;
}

// A definition belongs to one loaded instance. Opening the page binds every call
// to the lifetime of the mount and the selected connection.
export interface ModulePage {
  descriptor: PageDescriptor;
  open(current: () => boolean): { settings?: DynamicSettings; Panel?: Component };
}

export interface ModulePageSource {
  lookup(target: string, module: string): ModulePage | undefined;
}

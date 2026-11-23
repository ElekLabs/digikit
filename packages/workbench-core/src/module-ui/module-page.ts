import type { Component } from 'solid-js';
import type { ModuleDescriptor } from './descriptor';

export interface SettingValue {
  value?: string;
  revision: number;
}

export interface SettingsClient {
  get(key: string): Promise<SettingValue>;
  put(key: string, value: string, revision: number): Promise<SettingValue>;
  reset(key: string, revision: number): Promise<SettingValue>;
}

// A definition belongs to one loaded instance. Opening the page binds every call
// to the lifetime of the mount and the selected connection.
export interface ModulePage {
  descriptor: ModuleDescriptor;
  open(current: () => boolean): { settings?: SettingsClient; Panel?: Component };
}

export interface ModulePageSource {
  lookup(target: string, module: string): ModulePage | undefined;
}

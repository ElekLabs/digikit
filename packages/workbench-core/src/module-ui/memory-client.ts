// In-memory backing for preview targets and isolated client tests. Keep the
// record across module loads. Open a new client for each page instance.
import { checkText, type ModuleDescriptor } from './descriptor';
import type { SettingsClient, SettingValue } from './module-page';

export function createMemorySettings() {
  const entries = new Map<string, SettingValue>();
  const get = (key: string): SettingValue => ({ ...(entries.get(key) ?? { revision: 0 }) });
  function change(key: string, value: string | undefined, revision: number) {
    const before = get(key);
    if (before.revision !== revision) throw new Error('The setting changed. Read it again before saving.');
    if (before.value !== value) entries.set(key, { value, revision: revision + 1 });
    return get(key);
  }
  return { get, change };
}

export function createMemoryModuleClient(
  descriptor: ModuleDescriptor,
  store: ReturnType<typeof createMemorySettings>,
  handlers: Record<string, () => string | undefined>,
  current: () => boolean,
) {
  const check = () => {
    if (!current()) throw new Error('This module page has closed.');
  };
  function field(key: string) {
    check();
    const result = descriptor.settings.find((entry) => entry.id === key);
    if (!result) throw new Error('Unknown public setting.');
    return result;
  }
  const settings: SettingsClient = {
    async get(key) {
      field(key);
      return store.get(key);
    },
    async put(key, value, revision) {
      checkText(field(key), value);
      return store.change(key, value, revision);
    },
    async reset(key, revision) {
      field(key);
      return store.change(key, undefined, revision);
    },
  };
  return {
    settings,
    async call(operation: string) {
      check();
      if (!descriptor.operations.includes(operation) || !Object.hasOwn(handlers, operation))
        throw new Error('Unknown module operation.');
      return handlers[operation]!();
    },
  };
}

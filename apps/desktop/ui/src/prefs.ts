// Values kept per profile of the app, over the app's KeyValue (apps/web/src/storage.ts). A storage that cannot be used reads as nothing and
// drops writes: the value lasts until the window closes (the signal that holds it).
import type { KeyValue } from '../../../web/src/storage';

export function createPrefs(storage: KeyValue): {
  readPref(key: string): string | null;
  writePref(key: string, value: string | null): void;
} {
  return {
    readPref: (key) => storage.get(key),
    writePref(key, value) {
      if (value === null) storage.remove(key);
      else storage.set(key, value);
    },
  };
}

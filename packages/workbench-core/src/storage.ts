// The storage the apps keep their choices and big blobs in, behind two small interfaces so that a test (or another host) can give its own.
// small strings per profile (KeyValue): the browser's localStorage, or memory. The bytes (BlobStore) are in blob-store.ts.
// Only this file touches `localStorage` (oxlint `no-restricted-globals`); everything else gets a KeyValue.
import { createSignal } from 'solid-js';

/** Small strings by key. A store that cannot be used (a private window, blocked site data) reads as empty and drops writes. */
export interface KeyValue {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

/** The browser's localStorage; every call is guarded (the accessor itself can throw). */
export function localStorageKeyValue(): KeyValue {
  return {
    get(key) {
      try {
        return localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    set(key, value) {
      try {
        localStorage.setItem(key, value);
      } catch {
        // the value lasts until the window closes (the signal that holds it)
      }
    },
    remove(key) {
      try {
        localStorage.removeItem(key);
      } catch {
        // as above
      }
    },
  };
}

/** A KeyValue in memory (tests, and an app with no storage). `initial` seeds it; `data` shows what was written. */
export function memoryKeyValue(initial: Record<string, string> = {}): KeyValue & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    get: (key) => data.get(key) ?? null,
    set: (key, value) => void data.set(key, value),
    remove: (key) => void data.delete(key),
  };
}

/** A value kept across restarts of the app in `storage`: a signal that starts from `read(stored)` and writes each change ('' removes it). */
export function remembered<T extends string>(
  storage: KeyValue,
  key: string,
  read: (v: string | null) => T,
): [() => T, (v: T) => void] {
  const [value, set] = createSignal<T>(read(storage.get(key)));
  return [
    value,
    (v) => {
      set(() => v);
      if (v) storage.set(key, v);
      else storage.remove(key);
    },
  ];
}

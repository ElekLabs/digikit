// Bytes by key (BlobStore): one IndexedDB object store, or memory. Only this file touches `indexedDB` (oxlint `no-restricted-globals`); the
// code that keeps big values (the ready-state cache, the firmware hand-off) gets a BlobStore. No imports: it runs under Node too.

/** Bytes by key, asynchronous (IndexedDB holds more than localStorage can). */
export interface BlobStore {
  get(key: string): Promise<Uint8Array | undefined>;
  put(key: string, value: Uint8Array): Promise<void>;
  delete(key: string): Promise<void>;
}

/** A BlobStore in memory (tests, and the fallback where IndexedDB is not there). */
export function memoryBlobStore(): BlobStore {
  const m = new Map<string, Uint8Array>();
  return {
    get: async (k) => m.get(k),
    put: async (k, v) => void m.set(k, v),
    delete: async (k) => void m.delete(k),
  };
}

/** One IndexedDB object store (`database`, `objectStore`) of byte arrays. */
export function indexedDbBlobStore(database: string, objectStore: string): BlobStore {
  const open = () =>
    new Promise<IDBDatabase>((resolve, reject) => {
      const req = indexedDB.open(database, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(objectStore);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error ?? new Error('IndexedDB is not available'));
    });
  const run = async <T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> => {
    const db = await open();
    try {
      return await new Promise<T>((resolve, reject) => {
        const tx = db.transaction(objectStore, mode);
        const req = f(tx.objectStore(objectStore));
        tx.oncomplete = () => resolve(req.result);
        tx.onerror = () => reject(tx.error ?? new Error('IndexedDB write failed'));
        tx.onabort = () => reject(tx.error ?? new Error('IndexedDB aborted'));
      });
    } finally {
      db.close();
    }
  };
  return {
    get: async (key) => (await run<Uint8Array | undefined>('readonly', (s) => s.get(key))) ?? undefined,
    put: async (key, value) => void (await run('readwrite', (s) => s.put(value, key))),
    delete: async (key) => void (await run('readwrite', (s) => s.delete(key))),
  };
}

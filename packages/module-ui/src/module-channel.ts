import type { SettingError } from './setting-error.ts';
import type { QueryResult } from './query-result.ts';
import type { DescribedSchema } from './settings-schema.ts';

/** A stored setting as the device holds it: raw bytes (absent while unset) and the revision, or the reason the request failed. */
export type RawSetting =
  { ok: true; value: Uint8Array | undefined; revision: number } | { ok: false; error: SettingError };

/** One file of the module's data directory. */
export interface ModuleFileEntry {
  name: string;
  size: number;
}

/** A notification the module side sends without a request. */
export type ModuleChannelEvent = { kind: 'file-progress'; job: number; done: number; total: number };

/**
 * The only interface that crosses a sandbox boundary between a module page and the app. Every request is async and takes and returns
 * structured-clone data: no functions, class instances or signals. Notifications are `ModuleChannelEvent` messages delivered through
 * `subscribe`; a sandbox bridge carries them as messages. Typed settings, the files helper and the typed service client are page-side helpers built on it
 * (`createModuleClient`).
 */
export interface ModuleChannel {
  describeSettings(): Promise<DescribedSchema>;
  getSetting(id: string): Promise<RawSetting>;
  putSetting(id: string, bytes: Uint8Array, revision: number): Promise<RawSetting>;
  resetSetting(id: string, revision: number): Promise<RawSetting>;
  listFiles(): Promise<ModuleFileEntry[]>;
  readFile(name: string): Promise<Uint8Array>;
  /** Sends one file. Progress arrives as `file-progress` events carrying `job`. */
  sendFile(job: number, name: string, bytes: Uint8Array): Promise<void>;
  removeFile(name: string): Promise<void>;
  /** Calls the module's query `method` (1-based position in its service) with an encoded request; the result is reply bytes or the reason there are none. */
  callQuery(method: number, request: Uint8Array): Promise<QueryResult>;
  /** Delivers every event until the returned function is called. */
  subscribe(listener: (event: ModuleChannelEvent) => void): () => void;
}

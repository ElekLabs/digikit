// A ModuleChannel on plain memory for preview and tests. `createModuleClient` over it shares `ModuleSettings` (validation, scaling, labels,
// revisions) with the device adapter; the channel holds a values map and a revision per setting, and the module's files.
import type { MemoryQueryService } from './memory-query-service.ts';
import type { QueryError, QueryResult } from './query-result.ts';
import type { ModuleChannel, ModuleChannelEvent, RawSetting } from './module-channel.ts';
import { checkSaveFiles, type SaveFile } from './save-files.ts';
import { settingError, type SettingError } from './setting-error.ts';
import { settingValueCodec } from './setting-value-codec.ts';
import type { DescribedSchema, SettingsSchema } from './settings-schema.ts';
import { MemorySettingsStore } from './memory-settings-store.ts';

/** One call the test may hold back or refuse. */
export interface MemoryCall {
  method: 'get' | 'put' | 'reset' | 'files';
  id: string;
}

export interface MemoryOptions {
  /** The module name `describeSettings` reports. The default is the schema's name, then its module id. */
  name?: string;
  /** Display values stored before the first call, by setting id. */
  initial?: Record<string, string | number | boolean>;
  /** A store kept across channels (a page reload). The default is a fresh store. */
  store?: MemorySettingsStore;
  /** While false every call fails with `unavailable`: the instance or target the channel belonged to has gone. */
  current?: () => boolean;
  /** Runs before each call. Return a promise to hold the call (a pending state) or a `SettingError` to refuse it. */
  behavior?: (call: MemoryCall) => SettingError | void | Promise<SettingError | void>;
  /** The module's data directory. */
  files?: Map<string, Uint8Array>;
  /** The module's query service (`memoryQueryService(desc, handlers)`). Without one every query is an unknown method. */
  service?: MemoryQueryService;
  /** The device's byte limits of a request and a reply (the desktop preview passes the generated ones). A larger one answers `too-big`, as the device does. Without it nothing is limited. */
  queryLimits?: { request: number; reply: number };
}

/** What a test may look at beside the channel. A page never gets it. */
export interface MemoryInspect {
  readonly store: MemorySettingsStore;
  readonly files: Map<string, Uint8Array>;
  /** The saves the page asked for, in order, with the bytes copied. */
  readonly saved: SaveFile[][];
  /** Makes the next query fail with `busy` or `timeout` without reaching the handler. */
  failNextQuery(kind: 'busy' | 'timeout'): void;
}

const gone = () => settingError('unavailable', 'The module page has closed.');

export function createMemoryModuleChannel(
  schema: SettingsSchema,
  options: MemoryOptions = {},
): { channel: ModuleChannel; inspect: MemoryInspect } {
  const store = options.store ?? new MemorySettingsStore(schema);
  for (const [id, value] of Object.entries(options.initial ?? {})) {
    const setting = schema.settings.find((s) => s.id === id);
    if (!setting) throw new Error(`Unknown public setting ${id}.`);
    const stored = store.change(id, settingValueCodec.encode(setting, value), 0);
    if (!stored.ok) throw new Error(stored.error.message);
  }
  const data = options.files ?? new Map<string, Uint8Array>();
  const listeners = new Set<(event: ModuleChannelEvent) => void>();
  const current = options.current ?? (() => true);
  const admit = async (call: MemoryCall): Promise<SettingError | undefined> => {
    if (!current()) return gone();
    const refused = (await options.behavior?.(call)) || undefined;
    return refused ?? (current() ? undefined : gone());
  };
  const guarded = async (call: MemoryCall, run: () => RawSetting): Promise<RawSetting> => {
    const refused = await admit(call);
    return refused ? { ok: false, error: refused } : run();
  };
  async function open(call: MemoryCall) {
    const refused = await admit(call);
    if (refused) throw new Error(refused.message);
  }
  const saved: SaveFile[][] = [];
  let forced: QueryError | undefined;
  const described: DescribedSchema = {
    ...structuredClone(schema),
    name: options.name ?? schema.name ?? schema.module ?? 'Module',
  };
  const channel: ModuleChannel = {
    describeSettings: async () => structuredClone(described),
    getSetting: (id) => guarded({ method: 'get', id }, () => store.read(id)),
    putSetting: (id, value, revision) => guarded({ method: 'put', id }, () => store.change(id, value, revision)),
    resetSetting: (id, revision) => guarded({ method: 'reset', id }, () => store.change(id, undefined, revision)),
    async listFiles() {
      await open({ method: 'files', id: 'list' });
      return [...data].map(([name, bytes]) => ({ name, size: bytes.length }));
    },
    async readFile(name) {
      await open({ method: 'files', id: name });
      const bytes = data.get(name);
      if (!bytes) throw new Error(`No such file: ${name}`);
      return bytes.slice();
    },
    async sendFile(job, name, bytes) {
      await open({ method: 'files', id: name });
      data.set(name, bytes.slice());
      for (const listener of listeners)
        listener({ kind: 'file-progress', job, done: bytes.length, total: bytes.length });
    },
    async removeFile(name) {
      await open({ method: 'files', id: name });
      data.delete(name);
    },
    async saveFiles(files) {
      if (!current()) throw new Error(gone().message);
      checkSaveFiles(files);
      saved.push(files.map((f) => ({ name: f.name, bytes: f.bytes.slice() })));
      return { saved: files.length, cancelled: false, where: 'memory' };
    },
    async callQuery(method, request): Promise<QueryResult> {
      if (!current()) return { ok: false, error: { kind: 'unavailable' } };
      if (forced) {
        const error = forced;
        forced = undefined;
        return { ok: false, error };
      }
      const limits = options.queryLimits;
      if (limits && request.length > limits.request) return { ok: false, error: { kind: 'too-big' } };
      const result = (await options.service?.call(method, request)) ?? { ok: false, error: { kind: 'unknown-method' } };
      if (limits && result.ok && result.reply.length > limits.reply) return { ok: false, error: { kind: 'too-big' } };
      return structuredClone(result) as QueryResult;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
  return {
    channel,
    inspect: {
      store,
      files: data,
      saved,
      failNextQuery(kind) {
        forced = { kind };
      },
    },
  };
}

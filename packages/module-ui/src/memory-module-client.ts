// A ModuleClient on plain memory for preview and tests. It shares `ModuleSettings` (validation, scaling, labels, revisions) with the
// device adapter and differs only in the transport: a values map and a revision per setting.
import type { ModuleClient } from './module-client.ts';
import type { ModuleFileClient } from './module-file-client.ts';
import { settingError, type SettingError } from './setting-error.ts';
import { settingValueCodec } from './setting-value-codec.ts';
import type { SettingsSchema } from './settings-schema.ts';
import type { RawSetting, SettingsTransport } from './settings-transport.ts';
import { ModuleSettings } from './typed-settings.ts';

type Entry = { value: Uint8Array | undefined; revision: number };

/** The values of one simulated module instance. It outlives page loads: open a new client for each page instance over the same store. */
export class MemorySettingsStore {
  readonly #entries = new Map<string, Entry>();
  readonly schema: SettingsSchema;
  constructor(schema: SettingsSchema) {
    this.schema = schema;
  }

  read(id: string): RawSetting {
    if (!this.#setting(id)) return unknown(id);
    const entry = this.#entries.get(id) ?? { value: undefined, revision: 0 };
    return { ok: true, value: entry.value?.slice(), revision: entry.revision };
  }

  change(id: string, value: Uint8Array | undefined, revision: number): RawSetting {
    const setting = this.#setting(id);
    if (!setting) return unknown(id);
    const before = this.read(id) as Extract<RawSetting, { ok: true }>;
    if (before.revision !== revision) return fail('conflict', 'The setting changed. Read it again before saving.');
    if (value && !settingValueCodec.accepts(setting, value)) return fail('invalid', settingValueCodec.refusal(setting));
    const same = before.value === value || (before.value && value && before.value.join() === value.join());
    if (!same) this.#entries.set(id, { value: value?.slice(), revision: revision + 1 });
    return this.read(id);
  }

  #setting(id: string) {
    return this.schema.settings.find((s) => s.id === id);
  }
}

const unknown = (id: string) => fail('invalid', `Unknown public setting ${id}.`);
const fail = (code: SettingError['code'], message: string): RawSetting => ({
  ok: false,
  error: settingError(code, message),
});

/** One call the test may hold back or refuse. */
export interface MemoryCall {
  method: 'get' | 'put' | 'reset' | 'files';
  id: string;
}

export interface MemoryOptions {
  /** Display values stored before the first call, by setting id. */
  initial?: Record<string, string | number | boolean>;
  /** A store kept across clients (a page reload). The default is a fresh store. */
  store?: MemorySettingsStore;
  /** While false every call fails with `unavailable`: the instance or target the client belonged to has gone. */
  current?: () => boolean;
  /** Runs before each call. Return a promise to hold the call (a pending state) or a `SettingError` to refuse it. */
  behavior?: (call: MemoryCall) => SettingError | void | Promise<SettingError | void>;
  /** The module's data directory. */
  files?: Map<string, Uint8Array>;
}

const gone = () => settingError('unavailable', 'The module page has closed.');

export function createMemoryModuleClient(
  schema: SettingsSchema,
  options: MemoryOptions = {},
): ModuleClient & { store: MemorySettingsStore } {
  const store = options.store ?? new MemorySettingsStore(schema);
  for (const [id, value] of Object.entries(options.initial ?? {})) {
    const setting = schema.settings.find((s) => s.id === id);
    if (!setting) throw new Error(`Unknown public setting ${id}.`);
    const stored = store.change(id, settingValueCodec.encode(setting, value), 0);
    if (!stored.ok) throw new Error(stored.error.message);
  }
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
  const transport: SettingsTransport = {
    get: (id) => guarded({ method: 'get', id }, () => store.read(id)),
    put: (id, value, revision) => guarded({ method: 'put', id }, () => store.change(id, value, revision)),
    reset: (id, revision) => guarded({ method: 'reset', id }, () => store.change(id, undefined, revision)),
  };
  const data = options.files ?? new Map<string, Uint8Array>();
  const files: ModuleFileClient = {
    async list() {
      await open({ method: 'files', id: 'list' });
      return [...data].map(([name, bytes]) => ({ name, size: bytes.length }));
    },
    async read(name) {
      await open({ method: 'files', id: name });
      const bytes = data.get(name);
      if (!bytes) throw new Error(`No such file: ${name}`);
      return bytes.slice();
    },
    async send(name, bytes, progress) {
      await open({ method: 'files', id: name });
      data.set(name, bytes.slice());
      progress(bytes.length, bytes.length);
    },
    async remove(name) {
      await open({ method: 'files', id: name });
      data.delete(name);
    },
  };
  async function open(call: MemoryCall) {
    const refused = await admit(call);
    if (refused) throw new Error(refused.message);
  }
  return { store, files, settings: (wanted) => new ModuleSettings(wanted, transport) };
}

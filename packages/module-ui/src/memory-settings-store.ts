import { settingError, type SettingError } from './setting-error.ts';
import { settingValueCodec } from './setting-value-codec.ts';
import type { RawSetting } from './module-channel.ts';
import type { SettingsSchema } from './settings-schema.ts';

type Entry = { value: Uint8Array | undefined; revision: number };

/** The values of one simulated module instance. It outlives page loads: open a new channel for each page instance over the same store. */
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

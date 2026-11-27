import type { Setting } from './setting.ts';
import type { AnySettingValue, SettingsOf, SettingsSchema } from './settings-schema.ts';
import { settingValueCodec, InvalidSettingValue } from './setting-value-codec.ts';
import { settingError, type SettingError } from './setting-error.ts';
import type { ModuleChannel, RawSetting } from './module-channel.ts';

/** What a read or a change returns. `unset` is true while no value is stored: text then reads `null`, other kinds read their default. */
export type SettingResult<V> =
  { ok: true; value: V; revision: number; unset: boolean } | { ok: false; error: SettingError };

type Id<V> = keyof V & string;

/** The requests `ModuleSettings` needs. */
export type SettingsChannel = Pick<ModuleChannel, 'getSetting' | 'putSetting' | 'resetSetting'>;

export interface TypedSettings<V extends object> {
  get<K extends Id<V>>(id: K): Promise<SettingResult<V[K]>>;
  /** Stores `value`. `revision` defaults to the one this object last returned for `id`; a setting never read refuses with `not-read`. */
  put<K extends Id<V>>(id: K, value: Exclude<V[K], null>, revision?: number): Promise<SettingResult<V[K]>>;
  /** Removes the stored value. `revision` defaults as in `put`. */
  reset<K extends Id<V>>(id: K, revision?: number): Promise<SettingResult<V[K]>>;
}

/** The typed view of a schema over a module channel. */
export class ModuleSettings<S extends SettingsSchema> implements TypedSettings<SettingsOf<S>> {
  readonly #seen = new Map<string, number>();
  readonly #schema: S;
  readonly #channel: SettingsChannel;
  constructor(schema: S, channel: SettingsChannel) {
    this.#schema = schema;
    this.#channel = channel;
  }

  get<K extends Id<SettingsOf<S>>>(id: K) {
    return this.#run(id, (setting) => this.#channel.getSetting(setting.id));
  }

  put<K extends Id<SettingsOf<S>>>(id: K, value: Exclude<SettingsOf<S>[K], null>, revision?: number) {
    return this.#run(id, (setting) => {
      let bytes: Uint8Array;
      try {
        bytes = settingValueCodec.encode(setting, value as AnySettingValue);
      } catch (e) {
        if (!(e instanceof InvalidSettingValue)) throw e;
        return invalid(e.message);
      }
      return this.#expect(id, revision) ?? this.#channel.putSetting(setting.id, bytes, this.#revision(id, revision)!);
    });
  }

  reset<K extends Id<SettingsOf<S>>>(id: K, revision?: number) {
    return this.#run(
      id,
      (setting) => this.#expect(id, revision) ?? this.#channel.resetSetting(setting.id, this.#revision(id, revision)!),
    );
  }

  #revision(id: string, given?: number) {
    return given ?? this.#seen.get(id);
  }

  #expect(id: string, given?: number): Promise<RawSetting> | undefined {
    return this.#revision(id, given) === undefined
      ? Promise.resolve({ ok: false, error: settingError('not-read', 'Read the setting before changing it.') })
      : undefined;
  }

  async #run(id: string, call: (setting: Setting) => RawSetting | Promise<RawSetting>): Promise<SettingResult<never>> {
    const setting = this.#schema.settings.find((s) => s.id === id);
    if (!setting) return invalid(`Unknown public setting ${id}.`);
    const raw = await call(setting);
    if (!raw.ok) return raw;
    this.#seen.set(id, raw.revision);
    if (raw.value === undefined) {
      const value = setting.kind === 'text' || setting.kind === 'file-name' ? null : setting.default;
      return { ok: true, value: value as never, revision: raw.revision, unset: true };
    }
    try {
      return {
        ok: true,
        value: settingValueCodec.decode(setting, raw.value) as never,
        revision: raw.revision,
        unset: false,
      };
    } catch (e) {
      if (!(e instanceof InvalidSettingValue)) throw e;
      return { ok: false, error: settingError('invalid-stored', e.message) };
    }
  }
}

const invalid = (message: string): { ok: false; error: SettingError } => ({
  ok: false,
  error: settingError('invalid', message),
});

/** The settings of a schema known only at run time (the generic form): ids and values are not narrowed. */
export type DynamicSettings = TypedSettings<SettingsOf<SettingsSchema>>;

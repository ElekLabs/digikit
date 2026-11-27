import type { Setting } from './setting.ts';

/** The shape of the generated `schema` constant (a descriptor decoded from a device has the same shape plus `name`). */
export interface SettingsSchema {
  readonly module?: string;
  readonly version: 1 | 2;
  readonly settings: readonly Setting[];
}

/** The module name a descriptor carries. */
export interface DescribedSchema extends SettingsSchema {
  readonly name: string;
}

/** The value a page reads and writes for one setting: text `string | null` (null is unset), an integer as its display value, a boolean, a choice as its label. */
export type SettingValueOf<S extends Setting> = S extends { kind: 'text' | 'file-name' }
  ? string | null
  : S extends { kind: 'integer' }
    ? number
    : S extends { kind: 'boolean' }
      ? boolean
      : S extends { kind: 'choice'; choices: infer C extends readonly string[] }
        ? C[number]
        : never;

/** The values of a schema by setting id. */
export type SettingsOf<S extends SettingsSchema> = {
  [E in S['settings'][number] as E['id']]: SettingValueOf<E>;
};

export type AnySettingValue = string | number | boolean | null;

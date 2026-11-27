// One public setting of a module as `patchkit workbench-ts` writes it into `workbench/gen/settings.ts` and as
// `DescriptorDecoder` reads it back from the descriptor a device reports. The field names are the generated names.

export type Apply = 'immediate' | 'next-restart';

interface SettingBase {
  readonly id: string;
  readonly label: string;
  readonly apply: Apply;
}

export interface TextSetting extends SettingBase {
  readonly kind: 'text';
  readonly maxBytes: number;
}

export interface FileNameSetting extends SettingBase {
  readonly kind: 'file-name';
  readonly maxBytes: number;
  readonly extension: string;
  readonly resetLabel: string;
}

/** Stored as an i32; `decimals` scales it for display (stored 75 with decimals 2 shows 0.75). */
export interface IntegerSetting extends SettingBase {
  readonly kind: 'integer';
  readonly min: number;
  readonly max: number;
  readonly step: number;
  readonly decimals: number;
  readonly unit: string;
  readonly default: number;
}

export interface BooleanSetting extends SettingBase {
  readonly kind: 'boolean';
  readonly default: boolean;
}

export interface ChoiceSetting extends SettingBase {
  readonly kind: 'choice';
  readonly choices: readonly string[];
  readonly default: string;
}

export type Setting = TextSetting | FileNameSetting | IntegerSetting | BooleanSetting | ChoiceSetting;
export type SettingKind = Setting['kind'];

// The settings half of the device's ModuleChannel: a session's raw settings endpoint as RawSetting requests. dk-session moves bytes, module-ui
// owns the descriptor and value rules; the pairing lives here.
import { DkError, type ModuleSettingsEndpoint, type RawSettingValue } from '@digi/dk-session';
import {
  decodeDescriptor,
  settingError,
  type DescribedSchema,
  type ModuleChannel,
  type RawSetting,
  type SettingError,
} from '@digi/module-ui';

/** A session's settings endpoint. A simulated device (the preview) may give its schema directly instead of a descriptor. */
export type SettingsEndpoint = ModuleSettingsEndpoint & { describeSchema?(): Promise<DescribedSchema> };

export async function describeDeviceSettings(endpoint: SettingsEndpoint): Promise<DescribedSchema> {
  return endpoint.describeSchema ? endpoint.describeSchema() : decodeDescriptor(await endpoint.describe());
}

/** The typed error of a failed exchange. A status the device sends for a setting maps to its code; anything else means the module is out of reach. */
export function settingErrorOf(e: unknown): SettingError {
  const message = e instanceof Error ? e.message : String(e);
  const status = e instanceof DkError ? e.status : undefined;
  switch (status) {
    case 'E_MODULE_UI_CONFLICT':
      return settingError('conflict', message);
    case 'E_MODULE_UI_SCOPE':
      return settingError('overlay-scope', message);
    case 'E_AGAIN':
      return settingError('busy', message);
    case 'E_SET_VALUE':
    case 'E_MSG':
      return settingError('invalid', message);
    default:
      return settingError('unavailable', message);
  }
}

type SettingRequests = Pick<ModuleChannel, 'getSetting' | 'putSetting' | 'resetSetting'>;

export function deviceSettingRequests(schema: DescribedSchema, endpoint: ModuleSettingsEndpoint): SettingRequests {
  const field = (id: string) => schema.settings.findIndex((s) => s.id === id);
  async function run(id: string, call: (field: number) => Promise<RawSettingValue>): Promise<RawSetting> {
    const index = field(id);
    if (index < 0) return { ok: false, error: settingError('invalid', `Unknown public setting ${id}.`) };
    try {
      const { value, revision } = await call(index);
      return { ok: true, value, revision };
    } catch (e) {
      return { ok: false, error: settingErrorOf(e) };
    }
  }
  return {
    getSetting: (id) => run(id, (f) => endpoint.get(f)),
    putSetting: (id, value, revision) => run(id, (f) => endpoint.put(f, value, revision)),
    resetSetting: (id, revision) => run(id, (f) => endpoint.reset(f, revision)),
  };
}

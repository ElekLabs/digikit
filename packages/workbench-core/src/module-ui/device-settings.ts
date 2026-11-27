// The adapter between a session's raw settings endpoint and the typed settings of @digi/module-ui. dk-session moves bytes, module-ui owns the
// descriptor and value rules; the pairing lives here.
import { DkError, type ModuleSettingsEndpoint, type RawSettingValue } from '@digi/dk-session';
import {
  decodeDescriptor,
  ModuleSettings,
  settingError,
  type DescribedSchema,
  type DynamicSettings,
  type RawSetting,
  type SettingError,
  type SettingsTransport,
} from '@digi/module-ui';

export async function describeDeviceSettings(endpoint: ModuleSettingsEndpoint): Promise<DescribedSchema> {
  return decodeDescriptor(await endpoint.describe());
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

function transport(schema: DescribedSchema, endpoint: ModuleSettingsEndpoint): SettingsTransport {
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
    get: (id) => run(id, (f) => endpoint.get(f)),
    put: (id, value, revision) => run(id, (f) => endpoint.put(f, value, revision)),
    reset: (id, revision) => run(id, (f) => endpoint.reset(f, revision)),
  };
}

export function deviceSettings(schema: DescribedSchema, endpoint: ModuleSettingsEndpoint): DynamicSettings {
  return new ModuleSettings(schema, transport(schema, endpoint));
}

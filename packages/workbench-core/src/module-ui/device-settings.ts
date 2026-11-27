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

/** The settings requests of a module channel: what the device adapter and the memory channel both give. */
export type ModuleSettingsChannel = Pick<
  ModuleChannel,
  'describeSettings' | 'getSetting' | 'putSetting' | 'resetSetting'
>;

/** The typed error of a failed exchange. A status the device sends for a setting maps to its code; anything else means the module is out of reach. */
export function settingErrorOf(e: unknown, request: 'get' | 'change' = 'change'): SettingError {
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
      // A read the device refuses as E_SET_VALUE found a stored value that fails the setting's rule; a write refused so is a bad value.
      return settingError(request === 'get' ? 'invalid-stored' : 'invalid', message);
    case 'E_MSG':
      return settingError('invalid', message);
    default:
      return settingError('unavailable', message);
  }
}

/**
 * The settings requests of one loaded module instance over a session. The endpoint and the descriptor are fetched at the first request
 * (a failed fetch is tried again at the next); without an endpoint the module has no public settings.
 */
export function deviceSettingsChannel(
  id: string,
  endpoint?: () => Promise<ModuleSettingsEndpoint>,
): ModuleSettingsChannel {
  const none = async (): Promise<RawSetting> => ({
    ok: false,
    error: settingError('unavailable', 'The module has no public settings.'),
  });
  if (!endpoint)
    return {
      describeSettings: async () => ({ name: id, version: 1, settings: [] }),
      getSetting: none,
      putSetting: none,
      resetSetting: none,
    };
  let opened: Promise<{ point: ModuleSettingsEndpoint; schema: DescribedSchema }> | undefined;
  const open = () => {
    opened ??= (async () => {
      const point = await endpoint();
      return { point, schema: decodeDescriptor(await point.describe()) };
    })();
    void opened.catch(() => (opened = undefined));
    return opened;
  };
  async function run(
    setting: string,
    request: 'get' | 'change',
    call: (point: ModuleSettingsEndpoint, field: number) => Promise<RawSettingValue>,
  ): Promise<RawSetting> {
    try {
      const { point, schema } = await open();
      const field = schema.settings.findIndex((s) => s.id === setting);
      if (field < 0) return { ok: false, error: settingError('invalid', `Unknown public setting ${setting}.`) };
      const { value, revision } = await call(point, field);
      return { ok: true, value, revision };
    } catch (e) {
      return { ok: false, error: settingErrorOf(e, request) };
    }
  }
  return {
    describeSettings: async () => (await open()).schema,
    getSetting: (setting) => run(setting, 'get', (p, f) => p.get(f)),
    putSetting: (setting, value, revision) => run(setting, 'change', (p, f) => p.put(f, value, revision)),
    resetSetting: (setting, revision) => run(setting, 'change', (p, f) => p.reset(f, revision)),
  };
}

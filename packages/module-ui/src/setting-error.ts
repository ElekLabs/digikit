export type SettingErrorCode =
  /** The setting changed since the page read it. */
  | 'conflict'
  /** Project settings are active: the setting changes on the device. */
  | 'overlay-scope'
  /** The value breaks the setting's rule. */
  | 'invalid'
  /** The device has no room for the request now. */
  | 'busy'
  /** The module is not loaded, the page closed or the target went away. */
  | 'unavailable';

export interface SettingError {
  code: SettingErrorCode;
  message: string;
}

export const settingError = (code: SettingErrorCode, message: string): SettingError => ({ code, message });

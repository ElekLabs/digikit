export type SettingErrorCode =
  /** The setting changed since the page read it. */
  | 'conflict'
  /** Project settings are active: the setting changes on the device. */
  | 'overlay-scope'
  /** `put` or `reset` without a revision for a setting this object never read. */
  | 'not-read'
  /** The value breaks the setting's rule. */
  | 'invalid'
  /** The stored value fails the setting's rule, so a read cannot return it. A page shows the default. */
  | 'invalid-stored'
  /** The device has no room for the request now. */
  | 'busy'
  /** The module is not loaded, the page closed or the target went away. */
  | 'unavailable';

export interface SettingError {
  code: SettingErrorCode;
  message: string;
  /** The setting's revision, when the device sent it with the refusal. `invalid-stored` carries it so a page can overwrite the bad value. */
  revision?: number;
}

export const settingError = (code: SettingErrorCode, message: string, revision?: number): SettingError => ({
  code,
  message,
  ...(revision === undefined ? {} : { revision }),
});

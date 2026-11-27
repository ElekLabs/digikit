// The byte-level seam under `TypedSettings`: a device adapter (workbench-core) and the memory client implement it. Everything above it
// (validation, scaling, labels, the last-seen revision) lives once in `TypedSettings`.
import type { SettingError } from './setting-error.ts';

export type RawSetting =
  { ok: true; value: Uint8Array | undefined; revision: number } | { ok: false; error: SettingError };

export interface SettingsTransport {
  get(id: string): Promise<RawSetting>;
  put(id: string, value: Uint8Array, revision: number): Promise<RawSetting>;
  reset(id: string, revision: number): Promise<RawSetting>;
}

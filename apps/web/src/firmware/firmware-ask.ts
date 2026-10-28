// What the device on a session says of itself: SoftwareVersion (the stock firmware and the runtime both answer it) and INFO (the runtime only).
// Pure over the session, so the page, the headless run against the emulator and the tests ask the same way.
import { fmtVersion, type DkSession } from '@digi/dk-session';
import { parseSoftwareVersion, RPC_SOFTWARE_VERSION } from '../transfer/connection-core';
import type { Answer } from './firmware-core';

/** The model INFO names (`dt2`, `dn2`), when the runtime answered. Throws when neither answers. */
export async function askSession(s: Pick<DkSession, 'rpc' | 'info'>): Promise<Answer & { model?: string }> {
  const os = await s
    .rpc(RPC_SOFTWARE_VERSION, undefined, 1500)
    .then(parseSoftwareVersion)
    .catch(() => undefined);
  const info = await s.info(true).catch(() => undefined);
  if (!os && !info) throw new Error('The device does not answer.');
  return {
    ...(os ? { os } : {}),
    ...(info ? { runtime: fmtVersion(info.runtime), imageId: info.image_id, model: info.port } : {}),
  };
}

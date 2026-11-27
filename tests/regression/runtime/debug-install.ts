import type { PatchedSession } from '../test-runtime.ts';
import { DEBUG_MODULE } from './debug-module.ts';
import { moduleArtifact } from './module-artifact.ts';
import { installModules } from './module-install.ts';

/** The debug module and the modules `ids`, in one selection and one restart. */
export function installWithDebug({ session, dk }: PatchedSession, ...ids: string[]): Promise<void> {
  const device = session.options.device;
  return installModules(
    dk,
    [...new Set([DEBUG_MODULE, ...ids])].map((id) => ({ id, artifact: moduleArtifact(device, id) })),
    { device, session },
  );
}

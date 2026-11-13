import type { SessionOptions } from '@digi/test-support';
import { patchkit } from './patchkit.ts';
import { prepareOfflineWith, type PreparedRuntimePlan } from './runtime-plan.ts';

/**
 * Offline plan preparation for an explicit list of module roots (`apply --modules`). The result is not in dist/bytes/images.sha256, so a
 * test that imports this file counts as running patchkit live (its merge key keeps the patchkit binary).
 */
export function prepareRuntimePlanModulesOffline(
  options: Pick<SessionOptions, 'device' | 'firmware' | 'runtime'>,
  directory: string,
  artifacts: string,
  symbols: string[],
  selection: { modules: string[] },
): Promise<PreparedRuntimePlan> {
  return prepareOfflineWith(patchkit, options, directory, artifacts, symbols, selection);
}

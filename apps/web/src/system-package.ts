import type { PatchKitLike } from '@digi/workbench-core/patch/patchkit-core.ts';
import type { SystemPackage } from '@digi/workbench-core/runtime/activate-system-package.ts';
import type { WasmSource } from '@digi/workbench-core/wasm-source.ts';

/** Source-built package plus its runtime version, in the same envelope as the desktop host. */
export async function readWebSystemPackage(
  source: WasmSource,
  kit: () => Promise<PatchKitLike>,
): Promise<SystemPackage> {
  const buffer = await source.bytes('modules/system-package.bin');
  if (buffer.byteLength <= 4) throw new Error('The system package is missing or truncated. Rebuild the site.');
  const planner = await kit();
  return {
    runtime: new DataView(buffer).getUint32(0),
    bytes: new Uint8Array(buffer, 4),
    plan: async (input) => planner.systemPackagePlan(input),
  };
}

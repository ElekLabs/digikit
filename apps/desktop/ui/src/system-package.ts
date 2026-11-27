import type { Host } from '@digi/workbench-core/host/host.ts';
import type { SystemPackage } from '@digi/workbench-core/runtime/activate-system-package.ts';

export async function readSystemPackage(host: Host['firmware']): Promise<SystemPackage> {
  const plan = host.systemPackagePlan?.bind(host);
  if (!plan) throw new Error('Workbench did not provide a system package planner. Rebuild Workbench inputs.');
  const buffer = await host.systemPackage?.();
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength <= 4)
    throw new Error('Workbench did not receive a system package. Rebuild Workbench inputs.');
  return {
    runtime: new DataView(buffer).getUint32(0),
    bytes: new Uint8Array(buffer, 4),
    plan,
  };
}

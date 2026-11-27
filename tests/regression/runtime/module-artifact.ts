import fs from 'node:fs';
import path from 'node:path';
import { moduleStorePathOf, repoRoot, type Device } from '@digi/test-support';

/** Prepared separately with patchkit pack; acceptance never builds a toolchain, module or daemon. A service (`role = "service"`) is a `.dks`, any other module a `.dkm`. */
export function moduleArtifact(device: Device, id: string): string {
  const directory = process.env.DIGI_RUNTIME_MODULES ?? path.join(repoRoot(), 'dist/modules');
  const leaf = path.join(directory, device, id.split('/').at(-1)!);
  // a service (role 1) packs to a .dks, every other module to a .dkm
  const artifact = fs.existsSync(`${leaf}.dkm`) || !fs.existsSync(`${leaf}.dks`) ? `${leaf}.dkm` : `${leaf}.dks`;
  if (!fs.existsSync(artifact)) throw new Error(`missing ${artifact}: run nx run ${id}:pack`);
  return artifact;
}

/**
 * The store path of a prepared module: `modules/<kind>/<author>-<name>.dkm`, the one rule of Transfer (`modulePath`), the kind from the records of the packed file. `as` is the file name
 * a test chooses (`modules/<kind>/<as>.dkm`).
 */
export function moduleStorePath(device: Device, id: string, as?: string): string {
  return moduleStorePathOf(id, fs.readFileSync(moduleArtifact(device, id)), as);
}

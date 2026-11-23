import fs from 'node:fs';
import path from 'node:path';
import { repoRoot } from '@digi/test-support';
import type { Files } from './card-kit.ts';

/**
 * The system package on a test card: the one owner of where the file comes from and where it goes. `dist/syspkg/runtime/system.dksp` is written by the Nx target
 * `core/runtime:package` (a dependency of every device target); stage 1 of the resident core places it at boot from the store path `digikit/system.dksp`, and a card
 * without it boots core-only. Every card builder of the harness calls `installSystemPackage` on its seed folder, and `cardKey` holds `systemPackageSha`, so a changed
 * package gives new cards and so new boot snapshots.
 */

/** The package artifact. */
export const systemPackageFile = (): string => path.join(repoRoot(), 'dist/syspkg/runtime/system.dksp');

/** The store path stage 1 reads (`PACKAGE_PATH`, runtime/core/src/rs/src/stage1.rs): relative to the store root, which is the folder `digikit/` of the card. */
export const SYSTEM_PACKAGE_STORE_PATH = 'digikit/system.dksp';

/** Copy the package to its store path in `seed` (the card's `digikit/` folder holds the store). */
export function installSystemPackage(seed: string): void {
  const file = systemPackageFile();
  if (!fs.existsSync(file)) throw new Error(`missing ${file}: run nx run core/runtime:package`);
  const to = path.join(seed, 'digikit', SYSTEM_PACKAGE_STORE_PATH);
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(file, to);
}

/** The system package as a store file of a card that carries it: a test's expected listing starts from this. */
export const systemPackageFiles = (): Files => ({ [SYSTEM_PACKAGE_STORE_PATH]: fs.readFileSync(systemPackageFile()) });

/** The sectors the package takes in a store: its data and the table sector written beside it. */
export const systemPackageSectors = (): number => Math.ceil(fs.statSync(systemPackageFile()).size / 512) + 1;

/** CAPACITY's STORE `used` (KiB) of a store that holds the package only: the header sector and the package's sectors, rounded up. */
export const systemPackageUsedKib = (): number => Math.ceil((1 + systemPackageSectors()) / 2);

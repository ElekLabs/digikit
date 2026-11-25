import fs from 'node:fs';
import { repoRoot } from '@digi/test-support';
import {
  installSystemPackage as installPackage,
  systemPackagePath,
} from '../../../packages/workbench-core/src/node/card-image.ts';
import { SYSTEM_PACKAGE_PATH } from '../../../packages/workbench-core/src/runtime/system-package.ts';
import type { Files } from './card-kit.ts';

/**
 * The system package on a test card: the one owner of where the file comes from and where it goes. `dist/syspkg/runtime/system.dksp` is written by the Nx target
 * `core/runtime:package` (a dependency of every device target); stage 1 of the resident core places it at boot from the store path `digikit/system.dksp`, and a card
 * without it boots core-only. Every card builder of the harness calls `installSystemPackage` on its seed folder, and `cardKey` holds `systemPackageSha`, so a changed
 * package gives new cards and so new boot snapshots.
 */

/** The package artifact. */
export const systemPackageFile = (): string => systemPackagePath(repoRoot());

/** The store path stage 1 reads (`PACKAGE_PATH`, runtime/core/src/rs/src/stage1.rs): relative to the store root, which is the folder `digikit/` of the card. */
export const SYSTEM_PACKAGE_STORE_PATH = SYSTEM_PACKAGE_PATH;

/** Copy the package to its store path in `seed` (the card's `digikit/` folder holds the store). */
export function installSystemPackage(seed: string): void {
  installPackage(seed, systemPackageFile());
}

/** The system package as a store file of a card that carries it: a test's expected listing starts from this. */
export const systemPackageFiles = (): Files => ({ [SYSTEM_PACKAGE_STORE_PATH]: fs.readFileSync(systemPackageFile()) });

/** The sectors the package takes in a store: its data and the table sector written beside it. */
export const systemPackageSectors = (): number => Math.ceil(fs.statSync(systemPackageFile()).size / 512) + 1;

/** CAPACITY's STORE `used` (KiB) of a store that holds the package only: the header sector and the package's sectors, rounded up. */
export const systemPackageUsedKib = (): number => Math.ceil((1 + systemPackageSectors()) / 2);

/** Bytes of the file header before the body (`syspkg::Header`) and of the body header (`BodyHeader`, runtime/core/sdk/src/syspkg/body.rs). */
const FILE_HEADER_BYTES = 48;
const BODY_HEADER_BYTES = 32;

/** The bytes the package takes in its block (`BodyHeader::placed_bytes`: body header, image, the larger of the bss and the relocation table), read from the artifact's own body header: what CAPACITY reports as PACKAGE `used`. */
export function systemPackagePlacedBytes(): number {
  const f = fs.readFileSync(systemPackageFile());
  const word = (i: number): number => f.readUInt32BE(FILE_HEADER_BYTES + 4 * i);
  const [image, bss, relocs] = [word(1), word(2), word(3)] as [number, number, number];
  return BODY_HEADER_BYTES + image + Math.max(4 * relocs, bss);
}

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, resolveInputs, type Device } from '@digi/test-support';
import { FIRMWARE } from './devices.ts';

/** A service the card needs: its name (`services/<name>.dks`) and the module that provides it. */
export type CardService = { service: string; moduleId: string };

/**
 * Services that are files of the card (`services/<name>.dks`, docs/plans/runtime-and-services.md section 13.2). A module that needs one has a hard NEEDS item for its hash
 * (the packer adds it for a record kind of that service), and the runtime finds the file by its name at boot. The harness puts the file on the card of every test that
 * installs such a module, so a test never lists the service by hand. Which services a packed `.dkm` needs is `patchkit services` (the NEEDS and the manifests: no list
 * here); the Nx target `<module id>:pack` runs it and writes `<leaf>.services.json` beside the file, so a test only reads it and runs no patchkit.
 */
export function neededServices(files: string[]): CardService[] {
  const found = new Map<string, CardService>();
  for (const file of files) {
    const list = servicesSidecar(file);
    if (!fs.existsSync(list)) throw new Error(`missing ${list}: run nx run <module id>:pack`);
    for (const line of fs.readFileSync(list, 'utf8').split('\n').filter(Boolean)) {
      const v = JSON.parse(line) as { service: string; module_id: string };
      found.set(v.service, { service: v.service, moduleId: v.module_id });
    }
  }
  return [...found.values()].sort((x, y) => x.service.localeCompare(y.service));
}

/** The sidecar `<leaf>.services.json` that `neededServices` reads, for a packed `.dkm`/`.dks` (the Nx pack target writes it for shipped modules). */
export const servicesSidecar = (file: string) => file.replace(/\.dk[ms]$/, '.services.json');

/**
 * A card file under `modules/` that a test wrote as raw bytes with no packer behind it (a blob): it needs no service, and this writes that answer in the sidecar.
 * A copy or byte edit of a packed module is `derivedModule` instead; a module packed at test time gets its answer from `patchkit services` (synth.ts).
 */
export function rawModule(file: string): string {
  fs.writeFileSync(servicesSidecar(file), '');
  return file;
}

/**
 * A card file that a test made by copying or byte-editing the packed module `original` (a patch that leaves its NEEDS table alone): it needs what the original
 * needs, so it inherits the sidecar of the original. Throws when the original has none (it was not packed by the Nx pack target).
 */
export function derivedModule(original: string, copy: string): string {
  const from = servicesSidecar(original);
  if (!fs.existsSync(from)) throw new Error(`missing ${from}: run nx run <module id>:pack`);
  fs.copyFileSync(from, servicesSidecar(copy));
  return copy;
}

/** The store path of a service file. */
export const servicePath = (name: string) => `services/${name}.dks`;

/**
 * The packed service bound to the STOCK firmware of the device (`patchkit bind`): a service with hooks has trampolines and the stock bytes of its sites, so the packed
 * file is not enough. The Nx target `<module id>:bind` writes it under dist/ (keyed on the packed bytes and the firmware image; the file holds firmware bytes and is never
 * committed); a test only copies it, so no test that puts a service on a card is a live-patchkit task. `moduleFile` is the packed `.dks` (the `bind` input).
 */
export async function boundService(
  device: Device,
  { service: name, moduleId }: CardService,
  moduleFile: string,
): Promise<string> {
  const out = boundPath(device, name);
  const rerun = `run nx run ${moduleId}:bind`;
  if (!fs.existsSync(out)) throw new Error(`missing ${out}: ${rerun} (it binds ${moduleFile} to the stock firmware)`);
  // the sidecar of `patchkit bind` names what the file was bound from: a copy left in dist/ from other bytes is stale (mtimes say nothing under an Nx cache restore)
  const stale = staleBinding(out, moduleFile, resolveInputs(device, FIRMWARE[device]).syx);
  if (stale) throw new Error(`${out} ${stale}: ${rerun}`);
  return out;
}

const sha256 = (file: string) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');

/** Why the bound file `out` does not match the current packed `.dks` and firmware image (its `.src.json` sidecar), or undefined when it does. */
export function staleBinding(out: string, moduleFile: string, firmware: string): string | undefined {
  const sidecar = out.replace(/\.dks$/, '.src.json');
  if (!fs.existsSync(sidecar)) return 'has no .src.json sidecar (bound by an older patchkit)';
  const src = JSON.parse(fs.readFileSync(sidecar, 'utf8')) as { unbound_sha256?: string; firmware_sha256?: string };
  if (src.unbound_sha256 !== sha256(moduleFile)) return `was bound from another ${path.basename(moduleFile)}`;
  if (fs.existsSync(firmware) && src.firmware_sha256 !== sha256(firmware)) return 'was bound to another firmware image';
  return undefined;
}

/** The bound service file `dist/svc-bound/<device>/<name>.dks` (the Nx target `bind` writes it). */
export const boundPath = (device: Device, name: string) =>
  path.join(repoRoot(), 'dist/svc-bound', device, `${name}.dks`);

/** The address map of the bound file (the packed map moved by what bind appended; `patchkit bind --addrmap`). */
export const boundMapPath = (device: Device, name: string) =>
  boundPath(device, name).replace(/\.dks$/, '.addrmap.json');

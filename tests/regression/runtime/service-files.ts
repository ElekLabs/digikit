import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, type Device } from '@digi/test-support';
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

/** Header word H_DSP_SIZE of a .dkm or .dks: bytes of its DSP section (0: none). */
const H_DSP_SIZE = 0x9c;

/** The file has a DSP part (a service too: its dispatch code behind the hook stubs of the image). */
export const hasDspPart = (file: string): boolean => fs.readFileSync(file).readUInt32BE(H_DSP_SIZE) > 0;

/**
 * The one rule for service files on a card: the loader acknowledges a DSP part only from the DSP, and the zero peer and the cold boot never answer, so a service with a DSP part
 * (all-or-nothing: it then fails as a whole) needs `dsp: 'sharc'`. Throws, naming the file, instead of letting the test see a refused service.
 */
export function requireCoupledForServices(files: { name: string; file: string }[], dsp: string | undefined): void {
  if (dsp === 'sharc') return;
  const bad = files.filter((f) => f.name.startsWith('services/') && hasDspPart(f.file)).map((f) => f.name);
  if (bad.length)
    throw new Error(
      `${bad.join(', ')} ha${bad.length > 1 ? 've' : 's'} a DSP part, but ${dsp === undefined ? 'the DSP mode is not known here (pass the session)' : `this card runs with dsp:'${dsp}'`}: the test needs dsp:'sharc'`,
    );
}

/** The store path of a service file. */
export const servicePath = (name: string) => `services/${name}.dks`;

/**
 * The port file of the device's firmware: `dist/ports/<device>-<version>.dkport`, which the Nx target `patchkit:ports` builds from committed data (no firmware bytes). A service
 * file names the firmware by imports and hooks, and the runtime resolves them from the port file whose `FW` item equals the image's `fw_id`, so every card that holds a
 * service holds this file too, at `ports/<device>-<version>.dkport` (docs/plans/port-file.md). The service file itself is the packed `.dks`.
 */
export function portFile(device: Device): { name: string; file: string } {
  const leaf = `${device}-${FIRMWARE[device]}.dkport`;
  const file = path.join(repoRoot(), 'dist/ports', leaf);
  if (!fs.existsSync(file)) throw new Error(`missing ${file}: run nx run patchkit:ports`);
  return { name: `ports/${leaf}`, file };
}

/** The address map of the packed service file (`dist/modules/<device>/<name>.addrmap.json`, the Nx pack target writes it): the file loads as packed, so its offsets are the map's. */
export const serviceMapPath = (device: Device, name: string) =>
  path.join(process.env.DIGI_RUNTIME_MODULES ?? path.join(repoRoot(), 'dist/modules'), device, `${name}.addrmap.json`);

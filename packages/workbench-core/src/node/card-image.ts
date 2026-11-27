import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { SYSTEM_PACKAGE_PATH } from '../runtime/system-package.ts';

const exec = promisify(execFile);

type SeedFile = { name: string } & ({ file: string } | { bytes: Uint8Array });

interface CardImage {
  root: string;
  device: string;
  output: string;
  syx?: string;
  folder?: string;
  store?: readonly SeedFile[];
  drive?: readonly SeedFile[];
  systemPackage?: string;
  selection?: readonly string[];
  /** A caller-owned directory to keep for inspection. Otherwise a private seed is cleaned up. */
  seed?: string;
  timeoutMs?: number;
}

export function plusdrivePath(root: string): string {
  return (
    process.env.DIGI_PLUSDRIVE ||
    path.join(root, 'dist/plusdrive', process.platform === 'win32' ? 'plusdrive.exe' : 'plusdrive')
  );
}

export function systemPackagePath(root: string): string {
  return path.join(root, 'dist/syspkg/runtime/system.dksp');
}

function writeSeedFile(root: string, entry: SeedFile): void {
  const target = path.join(root, entry.name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  if ('file' in entry) fs.copyFileSync(entry.file, target);
  else fs.writeFileSync(target, entry.bytes);
}

export function installSystemPackage(seed: string, file: string): void {
  if (!fs.existsSync(file)) throw new Error(`missing ${file}: run nx run core/runtime:package`);
  writeSeedFile(path.join(seed, 'digikit'), { name: SYSTEM_PACKAGE_PATH, file });
}

/** Assemble a fresh base image. PlusDrive owns the verified superblock; callers own overlays and boot behavior. */
export async function buildCardImage(options: CardImage): Promise<void> {
  const { root, device, output, syx, folder, systemPackage, selection } = options;
  const seed = options.seed ?? fs.mkdtempSync(path.join(os.tmpdir(), 'digi-card-seed-'));
  try {
    if (options.seed) {
      fs.mkdirSync(path.dirname(seed), { recursive: true });
      fs.mkdirSync(seed);
    }
    if (folder) fs.cpSync(folder, seed, { recursive: true });
    else fs.mkdirSync(path.join(seed, 'digikit'));
    if (systemPackage) installSystemPackage(seed, systemPackage);
    for (const entry of options.store ?? []) writeSeedFile(path.join(seed, 'digikit'), entry);
    for (const entry of options.drive ?? []) writeSeedFile(seed, entry);
    if (selection !== undefined)
      writeSeedFile(path.join(seed, 'digikit'), {
        name: 'selection.txt',
        bytes: Buffer.from(selection.join('\n') + '\n'),
      });
    fs.mkdirSync(path.dirname(output), { recursive: true });
    await exec(
      plusdrivePath(root),
      ['build', seed, '-o', output, '--device', device, ...(syx ? ['--syx', syx] : ['--no-project'])],
      { cwd: root, timeout: options.timeoutMs ?? 60_000 },
    );
  } finally {
    if (!options.seed) fs.rmSync(seed, { recursive: true, force: true });
  }
}

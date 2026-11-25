import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cardCacheRoot, repoRoot, type Device } from '@digi/test-support';
import { plusdriveBinary } from './card-kit.ts';
import { systemPackageFile } from './system-package.ts';

/**
 * Content-named card images. The daemon's boot snapshot key holds the card's content id (the text of `card.key` next to the image), its
 * size and the sha256 of its `overlay.bin`; it holds no path or mtime. A card built here lives in `dist/session-cache/cards/<key>/`,
 * where the key is a hash of everything that goes into the card: the spec, the bytes of every input file, the plusdrive binary, the
 * device profiles and a format version. `card.key` holds that key. The same inputs give the same content id in every worktree, so the
 * snapshot of an earlier run, or of another worktree, is found. The files are read-only: a write to a shared card fails.
 */

/** Change it when the way a card is built changes in a way that the other key parts do not show. */
const FORMAT = 'card-cache-2';

const shaCache = new Map<string, { sig: string; sha: string }>();

/**
 * The files and directories of a plus_drive folder, sorted by `/` path, files with their sha256 and directories with `dir`: the walk of
 * `folder_files` (packages/emulator/plusdrive/src/folder.rs), which leaves out symbolic links and `.DS_Store` and keeps empty directories,
 * so a card key changes with exactly what `plusdrive build` reads.
 */
export function folderSha(root: string): { name: string; sha: string }[] {
  return fs
    .readdirSync(root, { recursive: true, withFileTypes: true })
    .filter((e) => (e.isFile() && e.name !== '.DS_Store') || e.isDirectory())
    .map((e) => {
      const file = path.join((e as { parentPath?: string }).parentPath ?? (e as { path: string }).path, e.name);
      return {
        name: path.relative(root, file).split(path.sep).join('/'),
        sha: e.isDirectory() ? 'dir' : fileSha(file),
      };
    })
    .sort((a, b) => (a.name < b.name ? -1 : 1));
}

/** The sha256 (hex) of a file, read once per path, size and mtime. */
export function fileSha(file: string): string {
  const stat = fs.statSync(file);
  const sig = `${stat.size}:${stat.mtimeMs}`;
  const known = shaCache.get(file);
  if (known?.sig === sig) return known.sha;
  const hash = createHash('sha256');
  const fd = fs.openSync(file, 'r');
  try {
    const buf = Buffer.alloc(1 << 20);
    for (let n; (n = fs.readSync(fd, buf, 0, buf.length, null)) > 0;) hash.update(buf.subarray(0, n));
  } finally {
    fs.closeSync(fd);
  }
  const sha = hash.digest('hex');
  shaCache.set(file, { sig, sha });
  return sha;
}

/** JSON with sorted object keys; bytes become their sha256. */
function canonical(value: unknown): string {
  return JSON.stringify(value, function (this: unknown, key: string, v: unknown) {
    const raw = (this as Record<string, unknown>)[key];
    if (raw instanceof Uint8Array) return { bytes: createHash('sha256').update(raw).digest('hex'), length: raw.length };
    if (v && typeof v === 'object' && !Array.isArray(v))
      return Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : 1)));
    return v;
  });
}

/** Card cache inputs include the plusdrive binary, the system package, device data and firmware port data. */
function toolIdentity(): { plusdrive: string; systemPackage: string; profiles: Record<string, string> } {
  const profiles: Record<string, string> = {};
  for (const tree of ['devices', 'ports']) {
    const dir = path.join(repoRoot(), tree);
    for (const name of fs.readdirSync(dir, { recursive: true, encoding: 'utf8' }).sort())
      if (name.endsWith('.toml') && (tree === 'devices' || !name.startsWith(`bindings${path.sep}`))) {
        profiles[path.join(tree, name)] = fileSha(path.join(dir, name));
      }
  }
  // the package every card carries: a changed package re-keys every card, so no snapshot of the old one is found
  const pkg = systemPackageFile();
  return {
    plusdrive: fileSha(plusdriveBinary()),
    systemPackage: fs.existsSync(pkg) ? fileSha(pkg) : 'absent',
    profiles,
  };
}

/**
 * The TypeScript files that build card specs and seed card files: the shared builder, this file, the card kit, the rigs that make the
 * specs (`card-rig`, `fx-session`, `fx-live`) and the helpers they use to lay out the card (services, plan, overlay, device data).
 */
const BUILDER_SOURCES = [
  '../../../packages/workbench-core/src/node/card-image.ts',
  '../../../packages/workbench-core/src/runtime/system-package.ts',
  'card-cache.ts',
  'card-kit.ts',
  'live-card.ts',
  'card-rig.ts',
  'fx-session.ts',
  'fx-live.ts',
  'service-files.ts',
  'system-package.ts',
  'service-symbols.ts',
  'runtime-plan.ts',
  'write-safety.ts',
  'device-data.ts',
  'project-kit.ts',
  'image-roots.ts',
  'fixtures.ts',
  'install.ts',
];

/** A hash of the bytes of the named files in `dir` (a missing file counts as absent). */
export function sourcesHash(dir: string, names: readonly string[]): string {
  const hash = createHash('sha256');
  for (const name of names) {
    const file = path.join(dir, name);
    hash.update(`${name}\0`);
    hash.update(fs.existsSync(file) ? fs.readFileSync(file) : 'absent');
    hash.update('\0');
  }
  return hash.digest('hex');
}

let builderHash: string | undefined;

/** The key of a card: `spec` is JSON-like, with each input file given as its sha256 (`fileSha`) and bytes as `Uint8Array`. */
export function cardKey(device: Device, spec: unknown): string {
  builderHash ??= sourcesHash(path.dirname(fileURLToPath(import.meta.url)), BUILDER_SOURCES);
  return createHash('sha256')
    .update(canonical({ format: FORMAT, device, spec, tools: toolIdentity(), builders: builderHash }))
    .digest('hex')
    .slice(0, 32);
}

/**
 * The key of a card of a rig spec. The files that the spec names count by content (`syx`, the module, drive and folder files), never by
 * path: a test that writes its drive files into a per-run temp dir still gets the same key in every run. `base` is the content id of
 * the base card (`cachedCardId`).
 */
export function cardSpecKey(
  device: Device,
  options: {
    syx?: string;
    modules: { name: string; file: string }[];
    drive?: { name: string; file: string }[];
    folder?: string;
    base?: unknown;
    [other: string]: unknown;
  },
): string {
  return cardKey(device, {
    ...options,
    syx: options.syx ? fileSha(options.syx) : undefined,
    modules: options.modules.map((m) => ({ name: m.name, sha: fileSha(m.file) })),
    drive: options.drive?.map((f) => ({ name: f.name, sha: fileSha(f.file) })),
    folder: options.folder ? folderSha(options.folder) : undefined,
  });
}

/**
 * Write the read-only `card.key` of a card dir. The text goes to a unique temp file that is renamed onto the target, so two workers never
 * write the same file. If the rename fails because another writer won, the existing text must equal ours.
 */
export function writeIdFile(idFile: string, key: string): void {
  const tmp = `${idFile}.${process.pid}.${Math.random().toString(36).slice(2)}.tmp`;
  fs.writeFileSync(tmp, key, { mode: 0o444 });
  try {
    fs.chmodSync(tmp, 0o444);
    fs.renameSync(tmp, idFile);
  } catch (error) {
    fs.rmSync(tmp, { force: true });
    let existing: string;
    try {
      existing = fs.readFileSync(idFile, 'utf8');
    } catch {
      throw error;
    }
    if (existing !== key) throw new Error(`${idFile} holds a different key (${existing}) than ${key}`);
  }
}

/** The key of a cached card, or undefined for a card outside the cache (a card that a test or the guest wrote). */
export function cachedCardId(card: string): { key: string; overlaySha: string } | undefined {
  const dir = path.dirname(card);
  if (path.dirname(dir) !== cardCacheRoot()) return undefined;
  const overlay = path.join(dir, 'overlay.bin');
  return { key: path.basename(dir), overlaySha: fs.existsSync(overlay) ? fileSha(overlay) : '' };
}

/**
 * The card for KEY. When its directory exists it is reused as it is (and touched, so eviction keeps it). Otherwise `produce` builds it in
 * a work directory and returns the `card.img` it made; the card's directory (`card.img` and `overlay.bin`) is made read-only and renamed
 * into place. Two workers that build the same key at once are safe: the one that loses the rename uses the winner's card.
 */
export async function cachedCard(key: string, produce: (work: string) => Promise<string>): Promise<string> {
  const root = cardCacheRoot();
  const final = path.join(root, key);
  const card = path.join(final, 'card.img');
  if (fs.existsSync(card)) {
    const now = new Date();
    fs.utimesSync(final, now, now);
    // A card dir of an earlier build has no content id yet; the dir name is the key, so this is the same text the build writes.
    const idFile = path.join(final, 'card.key');
    if (!fs.existsSync(idFile)) writeIdFile(idFile, key);
    return card;
  }
  fs.mkdirSync(root, { recursive: true });
  const work = fs.mkdtempSync(path.join(root, '.work-'));
  try {
    const made = await produce(work);
    const dir = path.dirname(made);
    // The content id the daemon puts in its snapshot key (see `card_identity` in packages/emulator/control).
    // atomic: a build through linkCard has already written a read-only card.key here
    writeIdFile(path.join(dir, 'card.key'), key);
    for (const name of ['card.img', 'overlay.bin']) {
      const file = path.join(dir, name);
      if (fs.existsSync(file)) fs.chmodSync(file, 0o444);
    }
    // The build's seed dir may stay in the card dir (tests read its files): read-only too.
    const seed = path.join(dir, 'seed');
    if (fs.existsSync(seed))
      for (const name of fs.readdirSync(seed, { recursive: true, encoding: 'utf8' })) {
        const file = path.join(seed, name);
        if (fs.statSync(file).isFile()) fs.chmodSync(file, 0o444);
      }
    try {
      fs.renameSync(path.dirname(made), final);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== 'ENOTEMPTY' && code !== 'EEXIST') throw error;
    }
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
  return card;
}

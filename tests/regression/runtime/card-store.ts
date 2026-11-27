import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { repoRoot, type Device } from '@digi/test-support';
import { currentRecord, encodeRecord, recordsIn, SECTOR, type SelRecord } from './card-record.ts';
import { cardStore } from './device-data.ts';
import { buildCardImage, plusdrivePath } from '../../../packages/workbench-core/src/node/card-image.ts';
import { systemPackageFile } from './system-package.ts';
import { sha } from './upload-bytes.ts';
import { cardOverlay, encodeOverlay, linkCard, mergeOverlays, overlayFile } from './write-safety.ts';

/** The card image and its store through the plusdrive CLI: build a card, list it, read its files back. */

const exec = promisify(execFile);

/** The plusdrive CLI: `DIGI_PLUSDRIVE` (a lane that built its own), else the shared build. */
export function plusdriveBinary(): string {
  return plusdrivePath(repoRoot());
}

// ---- the card image ----------------------------------------------------------------------------------------------------------------

export interface CardOptions extends Partial<Omit<SelRecord, 'paths'>> {
  /** Work directory: the seed folder and the card directory (`<name>/card.img` and `overlay.bin`) go here. */
  dir: string;
  name?: string;
  /** Modules of the store: `name` is the store path (`modules/a.dkm`), `file` the .dkm. */
  modules?: { name: string; file: string }[];
  /** Files of the DT2 +Drive path filesystem (`name` is the path below the root, e.g. `Samples/a.bin`; `file` the bytes). */
  drive?: { name: string; file: string }[];
  /** A plus_drive folder that `plusdrive build` scans as it is (the daemon's `card.load` scans a folder the same way); not with `modules` or `drive`. */
  folder?: string;
  /** `false`: leave `digikit/system.dksp` off the card (a test of a card without the package). Default: the built package is on it, also on a `folder` card. */
  systemPackage?: false;
  /** Verified DT2 firmware (.syx) that seeds the boot project; omit for none. */
  syx?: string;
  /** Selected store paths, written to both copies. Undefined or null: no record. */
  selection?: string[] | null;
  /** Each copy as given: raw bytes, null (left alone) or 'torn' (the record of the options with a bit flipped: its CRC fails). */
  recs?: [Buffer | null | 'torn', Buffer | null | 'torn'];
  /** Lay the records over a card that shares this card's image and keeps its writes, instead of building one; its sticky map is kept. */
  base?: string;
  /** Absolute sector -> bytes (a multiple of 512) written over the card, as overlay records. */
  foreign?: Record<number, Buffer>;
}

/** Python `Ctx.card` / `Rig.card`: a card with the store of `modules` and the record copies. Returns the card file. */
export async function buildCard(device: Device, o: CardOptions): Promise<string> {
  const dir = path.join(o.dir, o.name ?? 'card');
  const out = path.join(dir, 'card.img');
  fs.rmSync(dir, { recursive: true, force: true });
  if (o.folder && (o.modules?.length || o.drive?.length))
    throw new Error('buildCard: `folder` excludes `modules` and `drive`');
  if (!o.base) {
    const seed = path.join(o.dir, `seed-${o.name ?? 'card'}`);
    fs.rmSync(seed, { recursive: true, force: true });
    await buildCardImage({
      root: repoRoot(),
      device,
      output: out,
      syx: o.syx,
      folder: o.folder,
      store: o.modules,
      drive: o.drive,
      systemPackage: o.systemPackage !== false ? systemPackageFile() : undefined,
      seed,
    });
  }
  const sectors = new Map<number, Buffer>();
  if (o.selection != null || o.recs) {
    const map = o.map ?? (o.base ? currentRecord(recordsIn(o.base, device))?.map : undefined);
    const made = o.selection != null ? encodeRecord({ ...o, paths: o.selection, map }) : null;
    const torn = (rec: Buffer | null) => {
      const b = Buffer.from(rec ?? encodeRecord({ paths: [] }));
      b.writeUInt8(b.readUInt8(40) ^ 1, 40);
      return b;
    };
    const { selA, selB } = cardStore(device);
    (o.recs ?? [made, made]).forEach((r, i) => {
      const rec = r === 'torn' ? torn(made) : r;
      if (rec) sectors.set(i ? selB : selA, rec);
    });
  }
  for (const [first, data] of Object.entries(o.foreign ?? {}))
    for (let at = 0; at < data.length; at += SECTOR)
      sectors.set(Number(first) + at / SECTOR, data.subarray(at, at + SECTOR));
  const overlay = mergeOverlays(o.base ? cardOverlay(o.base) : Buffer.alloc(0), encodeOverlay(sectors));
  if (o.base) return linkCard(o.base, dir, overlay);
  fs.writeFileSync(overlayFile(out), overlay);
  return out;
}

/** The plusdrive option that reads a card through the `overlay.bin` next to its image. */
export const overlayArgs = (card: string): string[] =>
  fs.existsSync(overlayFile(card)) ? ['--overlay', overlayFile(card)] : [];

export interface StoreEntry {
  path: string;
  dir: boolean;
  firstSector: number;
  size: number;
  /** The entry's hash slot (hex), as `fileHashHex` gives it. */
  hash: string;
}

/** `plusdrive store ls`: the header line (used sectors, version, seq, header copy) and every entry with its hash slot, the system package of a card that carries it included. Fails on a card with no store. */
export async function storeList(cardImage: string, device: Device): Promise<{ head: string; entries: StoreEntry[] }> {
  const { stdout } = await exec(
    plusdriveBinary(),
    ['store', 'ls', cardImage, ...overlayArgs(cardImage), '--device', device],
    {
      cwd: repoRoot(),
    },
  );
  const [head, ...lines] = stdout.trim().split('\n');
  const entries = lines.map((line) => {
    const [p, first, size, sha] = line.split('\t') as [string, string, string, string];
    const dir = p.endsWith('/');
    return {
      path: dir ? p.slice(0, -1) : p,
      dir,
      firstSector: dir ? 0 : Number(first),
      size: Number(size),
      hash: dir ? '' : sha,
    };
  });
  return { head: head!, entries };
}

// ---- the store as the plusdrive reader sees it ----------------------------------------------------------------------------------------

export type Files = Record<string, Buffer>;
export type Listing = Record<string, [number, string]>;

/** {path: [size, hash]} of the files sent. */
export const listing = (files: Files): Listing =>
  Object.fromEntries(Object.entries(files).map(([p, b]) => [p, [b.length, sha(b)]]));

/** The files of a card's store as `plusdrive store ls --cat` reads them (their hash is checked there), and the reader's header line. A card the reader cannot read gives no files. */
export async function cardView(cardFile: string, device: Device): Promise<{ files: Files; head: string }> {
  let store: Awaited<ReturnType<typeof storeList>>;
  try {
    store = await storeList(cardFile, device);
  } catch (e) {
    return { files: {}, head: `error: ${String(e).slice(0, 200)}` };
  }
  const files: Files = {};
  for (const entry of store.entries) {
    if (entry.dir) continue;
    try {
      const cat = await exec(
        plusdriveBinary(),
        ['store', 'ls', cardFile, ...overlayArgs(cardFile), '--device', device, '--cat', entry.path],
        {
          cwd: repoRoot(),
          encoding: 'buffer',
          maxBuffer: 1 << 30,
        },
      );
      files[entry.path] = cat.stdout;
    } catch {
      files[entry.path] = Buffer.from('<unreadable>');
    }
  }
  return { files, head: store.head };
}

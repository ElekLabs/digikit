import fs from 'node:fs';
import path from 'node:path';
import type { Device } from '@digi/test-support';
import { cardStore, expansionRegion } from './device-data.ts';
import type { RuntimePlan } from './runtime-plan.ts';

/** Write-watch and card-overlay safety of the runtime proofs. */

// A `card.overlay` file holds, per sector written by the guest: u64 le sector, 64-byte mask (bit i of byte i / 8 names
// data byte i), 512 data bytes.
const SECTOR = 512;
const RECORD = 8 + 64 + SECTOR;
// Sectors of one settings copy (tools/device/dkproto.py SETTINGS_SECTORS).
const SETTINGS_SECTORS = 16;
// Kit-library sector ranges of the firmware (hi exclusive), allowed by the fx-kit and fx-routes policies.
export const KIT_AREAS: Record<Device, [number, number][]> = { dt2: [[0x16000, 0x17000]], dn2: [[0x2000, 0x2100]] };

export type SafetyPolicy = 'default' | 'fx-kit' | 'fx-routes';

export function* overlayRecords(blob: Buffer) {
  if (blob.length % RECORD) throw new Error(`overlay size ${blob.length} is not a multiple of ${RECORD}`);
  for (let at = 0; at < blob.length; at += RECORD)
    yield {
      sector: Number(blob.readBigUInt64LE(at)),
      mask: blob.subarray(at + 8, at + 72),
      data: blob.subarray(at + 72, at + RECORD),
    };
}

const overlaySectors = (file: string) => Array.from(overlayRecords(fs.readFileSync(file)), (r) => r.sector);

/** Sectors that lie in the module store of the device, sorted. */
function cardStoreHits(sectors: number[], device: Device): number[] {
  const { first, end } = cardStore(device);
  return sectors.filter((s) => first <= s && s < end).sort((a, b) => a - b);
}

/** Written module store sectors of an overlay, including the settings and selection copies. */
export function overlayWrites(overlay: string, device: Device): { store_sectors: number[] } {
  return { store_sectors: [...new Set(cardStoreHits(overlaySectors(overlay), device))] };
}

/** Copy BASE (a copy-on-write clone where the file system has one) to OUT and write the masked bytes of the overlay over it. */
export function applyOverlay(base: string, overlay: string, out: string): number {
  fs.rmSync(out, { force: true });
  fs.copyFileSync(base, out, fs.constants.COPYFILE_FICLONE);
  const fd = fs.openSync(out, 'r+');
  try {
    let n = 0;
    for (const { sector, mask, data } of overlayRecords(fs.readFileSync(overlay))) {
      const cur = Buffer.alloc(SECTOR);
      fs.readSync(fd, cur, 0, SECTOR, sector * SECTOR);
      for (let i = 0; i < SECTOR; i++) if (mask[i >> 3]! & (1 << (i & 7))) cur[i] = data[i]!;
      fs.writeSync(fd, cur, 0, SECTOR, sector * SECTOR);
      n++;
    }
    return n;
  } finally {
    fs.closeSync(fd);
  }
}

const hex = (n: number) => `0x${n.toString(16)}`;

/** The first symbol `<owner>@...:<name>` of the plan, if any. */
function ownedSymbol(plan: RuntimePlan, owner: string, name: string): number | undefined {
  for (const [key, value] of Object.entries(plan.symbols))
    if (key.startsWith(`${owner}@`) && key.slice(key.indexOf(':') + 1) === name) return value;
  return undefined;
}

/** The write-watch environment of the runtime proof: the module RAM minus the DMA holes of core/runtime, and the PCs of the modules that may write. */
export function runtimeWatchEnv(planFile: string, out: string): Record<string, string> {
  const plan: RuntimePlan & { regions: Record<string, number[]> } = JSON.parse(fs.readFileSync(planFile, 'utf8'));
  const [lo, hi] = expansionRegion(plan.base.device as Device, plan.base.version);
  // Card reads land by DMA in these buffers; an image of an older runtime lacks some of them.
  const holes: [number, number][] = [];
  const hole = (name: string, size: number) => {
    const start = ownedSymbol(plan, 'core/runtime', name);
    if (start !== undefined) holes.push([start, size]);
    return start !== undefined;
  };
  if (!hole('DKRT_STAGE', 64 * 1024)) throw new Error('no symbol core/runtime:DKRT_STAGE');
  hole('DKRT_SECBUF', 512);
  hole('DKRT_UPLOAD_BUF', 36896);
  hole('DKRT_STORE_PEEK', 512);
  hole('DKRT_STORE_FP', 512);
  // The header sector of the settings record is a hole only together with the record.
  if (hole('DKRT_SETTINGS', SETTINGS_SECTORS * SECTOR)) hole('DKRT_SETTINGS_HDR', 512);
  const ranges: string[] = [];
  let at = lo;
  for (const [start, size] of holes.sort((a, b) => a[0] - b[0] || a[1] - b[1])) {
    if (start > at) ranges.push(`${hex(at)}-${hex(start)}`); // adjacent holes leave no range between them
    at = start + size;
  }
  ranges.push(`${hex(at)}-${hex(hi)}`);
  return {
    DIGI_WRITE_WATCH: ranges.join(','),
    DIGI_WRITE_WATCH_AFTER: 'cacr',
    DIGI_WRITE_WATCH_ALLOW_PC: `${hex(lo)}-${hex(lo + plan.regions['cf.expansion']![0]!)}`,
    DIGI_FLUSH_CHECK: '1',
    DIGI_WRITE_WATCH_OUT: out,
    // One numbered log and summary per emulator, as runtimeSafety expects.
    DIGI_WRITE_WATCH_SEQ: '1',
  };
}

function runtimePolicy(device: Device, name: SafetyPolicy) {
  if (name !== 'default' && name !== 'fx-kit' && name !== 'fx-routes')
    throw new Error(`unknown runtime safety policy: ${String(name)}`);
  const store = cardStore(device);
  return {
    name,
    firmware_areas: name === 'default' ? [] : KIT_AREAS[device],
    store_extra: name === 'fx-routes' ? [store.setA, store.setB] : [],
  };
}

/** The card sectors an overlay may not contain: store sectors other than the selection copies and the policy's extras, and low sectors other than those and the kit areas. */
function runtimeCardPolicy(sectors: number[], device: Device, areas: [number, number][], extra: number[]) {
  const { selA, selB } = cardStore(device);
  const allowed = [selA, selB];
  const unique = [...new Set(sectors)];
  return {
    invalid_store: [...new Set(cardStoreHits(sectors, device))]
      .filter((s) => !allowed.includes(s) && !extra.includes(s))
      .sort((a, b) => a - b),
    invalid_low: unique
      .filter((s) => s < 0x50000 && !allowed.includes(s) && !areas.some(([lo, hi]) => lo <= s && s < hi))
      .sort((a, b) => a - b),
  };
}

/** After the owned processes exit: every sequenced watch log needs a complete clean summary, and every overlay a clean card policy. */
export function runtimeSafety(
  device: Device,
  output: string,
  overlays: string[],
  minEmulators = 1,
  policy: SafetyPolicy = 'default',
) {
  if (minEmulators < 1 || !overlays.length)
    throw new Error('runtime safety needs observed emulators and exported overlays');
  const facts = runtimePolicy(device, policy);
  const { dir, name: stem } = path.parse(output);
  const indexed = (suffix: string) => {
    const files = new Map<number, string>();
    for (const file of fs.readdirSync(dir)) {
      if (!file.startsWith(`${stem}-`) || !file.endsWith(`.${suffix}`)) continue;
      const index = file.slice(stem.length + 1, file.length - suffix.length - 1);
      if (!/^\d+$/.test(index)) throw new Error(`malformed watch sequence filename: ${path.join(dir, file)}`);
      files.set(Number(index), path.join(dir, file));
    }
    return files;
  };
  const logs = indexed('jsonl');
  const summaries = indexed('summary.json');
  if (
    logs.size < minEmulators ||
    [...logs.keys()].some((k) => k >= logs.size) ||
    logs.size !== summaries.size ||
    [...logs.keys()].some((k) => !summaries.has(k))
  )
    throw new Error('missing or incomplete sequenced emulator summaries');
  let events = 0;
  let flush = 0;
  let cpushl = 0;
  for (const file of summaries.values()) {
    const data = JSON.parse(fs.readFileSync(file, 'utf8')) as Record<string, unknown>;
    for (const key of ['events', 'cpushl'] as const)
      if (!Number.isInteger(data[key]) || (data[key] as number) < 0) throw new Error(`invalid ${key} in ${file}`);
    if (!Array.isArray(data.flush_hits)) throw new Error(`invalid flush_hits in ${file}`);
    events += data.events as number;
    flush += data.flush_hits.length;
    cpushl += data.cpushl as number;
  }
  const cards = overlays.map((file) => {
    const sectors = overlaySectors(file);
    return {
      path: file,
      sectors: sectors.length,
      ...runtimeCardPolicy(sectors, device, facts.firmware_areas, facts.store_extra),
    };
  });
  return {
    policy: facts,
    emulators: summaries.size,
    write_events: events,
    flush_hits: flush,
    cpushl,
    overlays: cards,
    pass: events === 0 && flush === 0 && cards.every((c) => !c.invalid_store.length && !c.invalid_low.length),
  };
}

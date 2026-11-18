// The backend of a device whose samples are numbered slots written through the stock MidiRpc Data API (`[storage] samples = "stock-slots"`, finding 142).
// 64 slots, no folders, no rename, no content hash in the listing, no bulk link. A sample goes into the lowest free slot or into the slot the caller names; an
// occupied slot is never overwritten unless named. Written without TS-only runtime syntax (no parameter properties) so node can import it.
import { DkError, fileHashHex } from './dkupload-core.ts';
import type { Prepared, SampleConverter, SlotFormat } from './dksample.ts';
import {
  BACKUP_HEADER,
  SLOT_COUNT,
  SLOT_HEADER,
  SLOT_IMAGE_MAX,
  checkSlot,
  unwrapBackup,
  wrapBackup,
} from './backup-file.ts';
import { RpcData, SLOT_ROUTE } from './rpcdata.ts';
import type { SlotRow } from './rpcdata.ts';
import type { RpcPort } from './rpcfiles.ts';
import type {
  PutOptions,
  RemoveTreeOptions,
  SampleEntry,
  SamplesBackend,
  SampleVerified,
  SlotsOptions,
} from './samples.ts';

const usage = (error: string) => new DkError({ error, kind: 'usage' });
const noFolders = () => usage('this device has numbered sample slots, no folders');
const ambiguous = (key: string, n: number) => usage(`ambiguous name: ${n} slots are listed as ${key}`);
const isRoot = (folder: string | undefined) => !folder || /^\/*$/.test(folder);

/** The unique key of each used slot: its name when only that slot has it, else `name #slot` (`slot #N` for an empty name). */
function keyed(rows: readonly SlotRow[]): { key: string; row: SlotRow }[] {
  const used = rows.filter((r) => r.used);
  const count = new Map<string, number>();
  for (const r of used) count.set(r.name, (count.get(r.name) ?? 0) + 1);
  return used
    .map((row) => ({
      key: row.name && count.get(row.name) === 1 ? row.name : `${row.name || 'slot'} #${row.slot}`,
      row,
    }))
    .sort((a, b) => a.row.slot - b.row.slot);
}

export class SlotSamples implements SamplesBackend {
  readonly kind = 'stock-slots' as const;
  private data: RpcData;
  private converter: SampleConverter;
  private format: SlotFormat;
  private count: number;
  /** The slot each prepared file went into (verify reads that slot). */
  private placed = new WeakMap<Prepared, number>();
  constructor(port: RpcPort, slots: SlotsOptions, converter: SampleConverter) {
    if (slots.path !== SLOT_ROUTE) throw usage(`this client writes ${SLOT_ROUTE} only, not ${slots.path}`);
    if (!Number.isInteger(slots.count) || slots.count < 1 || slots.count > SLOT_COUNT)
      throw usage(`slot count ${slots.count}: 1 to ${SLOT_COUNT}`);
    if (
      !Number.isInteger(slots.bytesMax) ||
      slots.bytesMax < 2 ||
      slots.bytesMax % 2 !== 0 ||
      slots.bytesMax + SLOT_HEADER > SLOT_IMAGE_MAX
    )
      throw usage(
        `bytes_max ${slots.bytesMax} does not fit a slot of ${SLOT_IMAGE_MAX} bytes with its ${SLOT_HEADER}-byte header`,
      );
    this.data = new RpcData(port);
    this.converter = converter;
    this.format = slots;
    this.count = slots.count;
  }

  prepare(name: string, bytes: Uint8Array) {
    return this.converter.slot(name, bytes, this.format);
  }
  /** The size of the slot image: what is stored and what the progress counts. */
  sizeOf(file: Prepared) {
    return file.bytes.length;
  }
  checksAtBulkEnd() {
    return false;
  }

  private async rows() {
    return (await this.data.list()).filter((r) => r.slot <= this.count);
  }

  async list(folder = '') {
    if (!isRoot(folder)) return [];
    return keyed(await this.rows()).map(({ key, row }): SampleEntry => ({
      name: key,
      kind: 'file',
      size: Math.max(0, row.size - SLOT_HEADER),
      stored: row.size,
      slot: row.slot,
    }));
  }
  async space() {
    const free = (await this.rows()).filter((r) => !r.used).length;
    return { free: free * this.format.bytesMax, total: this.count * this.format.bytesMax };
  }
  async canMkdir() {
    return false;
  }
  async mkdir(_folder: string): Promise<void> {
    throw noFolders();
  }
  async rmdir(_folder: string): Promise<void> {
    throw noFolders();
  }
  async removeTree(_folder: string, _o?: RemoveTreeOptions): Promise<never> {
    throw noFolders();
  }
  async canRename() {
    return false;
  }
  async rename(_folder: string, _name: string, _toFolder: string, _toName: string): Promise<void> {
    throw usage('this device cannot rename samples');
  }

  /** The slot listing has no content hash: a same name and size proves nothing, so every file is sent (verify reads the slot back). */
  sameOnDevice(_file: Prepared, _listing: readonly SampleEntry[]) {
    return false;
  }

  async put(
    folder: string,
    file: Prepared,
    onProgress?: (sent: number, total: number) => void,
    opts: PutOptions = {},
  ): Promise<void> {
    if (!isRoot(folder)) throw noFolders();
    // everything that can be refused is refused here, before the first request
    if (opts.slot !== undefined) {
      checkSlot(opts.slot);
      if (opts.slot > this.count) throw usage(`sample slot ${opts.slot}: this device has ${this.count}`);
    }
    if (file.bytes.length - SLOT_HEADER > this.format.bytesMax)
      throw usage(`the sample is ${file.bytes.length - SLOT_HEADER} audio bytes: at most ${this.format.bytesMax}`);
    wrapBackup(opts.slot ?? 1, file.bytes); // checks size, header and length word
    const slot = opts.slot ?? (await this.rows()).find((r) => !r.used)?.slot;
    if (slot === undefined)
      throw new DkError({
        error: `all ${this.count} sample slots are used: delete one, or name the slot to replace`,
        kind: 'device',
        status: 'E_FULL',
      });
    const wrapped = wrapBackup(slot, file.bytes);
    const total = file.bytes.length;
    await this.data.writeFile(
      slot,
      wrapped,
      (sent) => onProgress?.(Math.min(total, Math.max(0, sent - BACKUP_HEADER)), total),
      opts.signal,
    );
    this.placed.set(file, slot);
  }

  async verify(folder: string, file: Prepared): Promise<SampleVerified> {
    if (!isRoot(folder)) throw noFolders();
    const sent = file.bytes;
    const host = fileHashHex(sent);
    const base = {
      host,
      sent: sent.length,
      compared: 'the slot read back over the Data API (chunk and footer CRC checked), byte by byte with the bytes sent',
    };
    let slot = this.placed.get(file);
    if (slot === undefined) {
      const hits = keyed(await this.rows()).filter(
        ({ key, row }) => (key === file.name || key.startsWith(`${file.name} #`)) && row.size === sent.length,
      );
      if (hits.length > 1) throw ambiguous(file.name, hits.length);
      slot = hits[0]?.row.slot;
    }
    if (slot === undefined)
      return {
        ...base,
        ok: false,
        device: null,
        size: null,
        mismatchAt: null,
        message: 'the device does not have the file',
      };
    const got = unwrapBackup(await this.data.readFile(slot));
    if (got.slot !== slot) throw new DkError({ error: `slot ${slot} answered as slot ${got.slot}`, kind: 'protocol' });
    let at = -1;
    const n = Math.max(got.image.length, sent.length);
    for (let i = 0; i < n; i++)
      if (got.image[i] !== sent[i]) {
        at = i;
        break;
      }
    const ok = at < 0;
    return {
      ...base,
      ok,
      device: fileHashHex(got.image),
      size: Math.max(0, got.image.length - SLOT_HEADER),
      mismatchAt: ok ? null : at,
      message: ok ? 'the slot holds the bytes sent' : `the slot differs from the bytes sent at byte ${at}`,
    };
  }

  /** Clear the slot that `name` (as `list` gives it) names now; the listing is read again, so a name that is gone fails and never clears another slot. */
  async remove(folder: string, name: string): Promise<void> {
    if (!isRoot(folder)) throw noFolders();
    const hits = keyed(await this.rows()).filter((k) => k.key === name);
    if (hits.length > 1) throw ambiguous(name, hits.length);
    const hit = hits[0];
    if (!hit) throw new DkError({ error: `no sample "${name}" on the device`, kind: 'device', status: 'E_NOFILE' });
    await this.data.clear(hit.row.slot);
    const after = (await this.rows()).find((r) => r.slot === hit.row.slot);
    if (after?.used) throw new DkError({ error: `slot ${hit.row.slot} is still used after Clear`, kind: 'device' });
  }
}

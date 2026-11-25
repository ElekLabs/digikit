import type { DkSession } from '@digi/dk-session';
import type { Sample } from '@digi/dk-session/dksample';
import { parse } from 'smol-toml';

export interface NativeSampleKey {
  readonly content_hash: string;
  readonly stored_bytes: number;
}

/** Bind the DT2 1.17 reader to the resolved port for the target's verified stock firmware. */
export function dt2StockSampleReader(portSource: string, stockSha256: string) {
  const { port, anchor } = parse(portSource);
  if (!table(port) || port.version !== '1.17' || port.syx_sha256 !== stockSha256)
    throw new Error('The sample key port does not match the verified DT2 1.17 firmware.');
  const row = Array.isArray(anchor)
    ? anchor.find((a) => table(a) && a.name === 'cf.samples.site.slot.keys')
    : undefined;
  const address = table(row) ? row.addr : undefined;
  if (
    !table(row) ||
    row.cpu !== 'cf' ||
    row.len !== 4 ||
    typeof address !== 'number' ||
    address <= 0 ||
    address > 0xffff_fffc ||
    !Number.isInteger(address)
  )
    throw new Error('No stock sample key table anchor for DT2 1.17.');

  return {
    readLoadedKey: (session: Pick<DkSession, 'peek'>, slot: number) => readLoadedKey(session, address, slot),
  };
}

function table(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** The loader's resolved key, not the project's serialized reference or the DSP allocation. */
async function readLoadedKey(
  session: Pick<DkSession, 'peek'>,
  tableLiteral: number,
  slot: number,
): Promise<NativeSampleKey | null> {
  const keySize = 16;
  const slotCount = 1024;
  if (!Number.isInteger(slot) || slot < 0 || slot >= slotCount) throw new Error(`Invalid stock sample slot ${slot}.`);
  if (slot % 128 === 0) return null; // index zero in any bank selects no sample

  const view = async (addr: number, size: number) => {
    const bytes = await session.peek(addr, size);
    if (bytes.byteLength !== size) throw new Error('Incomplete stock sample key read.');
    return new DataView(bytes.buffer, bytes.byteOffset, size);
  };
  const base = (await view(tableLiteral, 4)).getUint32(0);
  if (base === 0 || base % 4 !== 0 || base + slotCount * keySize > 2 ** 32)
    throw new Error('Invalid stock sample key table pointer.');
  const key = await view(base + slot * keySize, keySize);
  if (key.getUint32(0) === 0xffff_ffff) return null;
  const hash = key.getUint32(4);
  const storedBytes = key.getUint32(8);
  if (!(hash & 1) || storedBytes === 0) throw new Error(`Invalid loaded sample key in slot ${slot}.`);
  return { content_hash: hash.toString(16).padStart(8, '0'), stored_bytes: storedBytes };
}

export function matchesNativeSample(actual: NativeSampleKey | null, expected: NativeSampleKey): boolean {
  return actual?.content_hash === expected.content_hash && actual.stored_bytes === expected.stored_bytes;
}

export function identifyNativeSample(converter: Pick<Sample, 'convert' | 'hash'>, wav: Uint8Array): NativeSampleKey {
  const native = converter.convert(wav).file; // The file includes the trailer added during upload.
  return { content_hash: converter.hash(native).toString(16).padStart(8, '0'), stored_bytes: native.byteLength };
}

// Hand-off of a patched firmware from /patch to the emulator page, inside this browser only:
// the bytes go into IndexedDB (too big for sessionStorage) and the emulator page takes and deletes them.
// Nothing is sent over the network. The store is a BlobStore (blob-store.ts): a test gives `memoryBlobStore()`.
import { indexedDbBlobStore, type BlobStore } from '../blob-store';

const KEY = 'pending';

export interface Handoff {
  name: string;
  device?: string;
  bytes: ArrayBuffer;
}

/** The browser's store for the hand-off. */
export const handoffStore = (): BlobStore => indexedDbBlobStore('digikit-handoff', 'firmware');

/** A hand-off as bytes: 4 bytes of header length (big endian), the header as JSON (name, device), the firmware. */
function encode(value: Handoff): Uint8Array {
  const header = new TextEncoder().encode(JSON.stringify({ name: value.name, device: value.device }));
  const out = new Uint8Array(4 + header.length + value.bytes.byteLength);
  new DataView(out.buffer).setUint32(0, header.length);
  out.set(header, 4);
  out.set(new Uint8Array(value.bytes), 4 + header.length);
  return out;
}

function decode(bytes: Uint8Array): Handoff {
  const n = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + n))) as { name: string; device?: string };
  return { ...header, bytes: bytes.slice(4 + n).buffer };
}

export const putHandoff = (store: BlobStore, value: Handoff): Promise<void> => store.put(KEY, encode(value));

/** Reads and deletes the pending hand-off (undefined when there is none). */
export async function takeHandoff(store: BlobStore): Promise<Handoff | undefined> {
  const stored = await store.get(KEY);
  if (!stored) return undefined;
  await store.delete(KEY);
  return decode(stored);
}

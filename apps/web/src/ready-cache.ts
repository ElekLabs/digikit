// The ready-state cache: what a cold boot of a user's own .syx produced (the
// DSP image and state and the ColdFire snapshot at ready), kept in the user's
// browser (IndexedDB) so the next visit starts in a moment. It is keyed by the
// SHA-256 of the .syx plus a version of the core that made it. Nothing here
// is ever uploaded or put in the site: it is firmware-derived and stays in
// the visitor's own storage.
//
// Only erasable TypeScript (the logic is tested under Node, tools/devtools/wasm_split.mjs
// users and apps/web/tests/ready-cache.test.mjs, with an in-memory store).

import type { BlobStore } from './blob-store.ts';

export interface ReadyState {
  /** Packed DSP image, DSP state (`DT2DSP01` file) and ColdFire snapshot (`DT2SNP01`). */
  image: Uint8Array;
  dsp: Uint8Array;
  snapshot: Uint8Array;
}

/** Bump when the core's state formats or its DSP initialisation change. */
export const CORE_VERSION = 1;

export async function sha256Hex(bytes: BufferSource): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function pipe(bytes: Uint8Array, stream: CompressionStream | DecompressionStream): Promise<Uint8Array> {
  const out = new Response(new Blob([bytes as BlobPart]).stream().pipeThrough(stream));
  return new Uint8Array(await out.arrayBuffer());
}

const MAGIC = 0x52445953; // "RDYS"

function pack(state: ReadyState): Uint8Array {
  const parts = [state.image, state.dsp, state.snapshot];
  const out = new Uint8Array(8 + 12 + parts.reduce((n, p) => n + p.length, 0));
  const dv = new DataView(out.buffer);
  dv.setUint32(0, MAGIC, true);
  dv.setUint32(4, CORE_VERSION, true);
  let at = 8;
  parts.forEach((p) => {
    dv.setUint32(at, p.length, true);
    at += 4;
  });
  parts.forEach((p) => {
    out.set(p, at);
    at += p.length;
  });
  return out;
}

function unpack(bytes: Uint8Array): ReadyState | undefined {
  if (bytes.length < 20) return undefined;
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (dv.getUint32(0, true) !== MAGIC || dv.getUint32(4, true) !== CORE_VERSION) return undefined;
  const lens = [dv.getUint32(8, true), dv.getUint32(12, true), dv.getUint32(16, true)];
  let at = 20;
  if (at + lens[0] + lens[1] + lens[2] !== bytes.length) return undefined;
  const [image, dsp, snapshot] = lens.map((n) => {
    const v = bytes.slice(at, at + n);
    at += n;
    return v;
  });
  return { image, dsp, snapshot };
}

export class ReadyCache {
  private store: BlobStore;
  constructor(store: BlobStore) {
    this.store = store;
  }

  static key(syxSha256: string): string {
    return `ready/v${CORE_VERSION}/${syxSha256}`;
  }

  async load(syxSha256: string): Promise<ReadyState | undefined> {
    const stored = await this.store.get(ReadyCache.key(syxSha256));
    if (!stored) return undefined;
    try {
      return unpack(await pipe(stored, new DecompressionStream('gzip')));
    } catch {
      await this.store.delete(ReadyCache.key(syxSha256));
      return undefined;
    }
  }

  async save(syxSha256: string, state: ReadyState): Promise<number> {
    const packed = await pipe(pack(state), new CompressionStream('gzip'));
    await this.store.put(ReadyCache.key(syxSha256), packed);
    return packed.length;
  }
}

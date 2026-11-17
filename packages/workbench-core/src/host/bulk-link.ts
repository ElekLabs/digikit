// The USB bulk link of the desktop app: the host's `ports.bulk` (the Tauri commands of `bulk_port.rs`, nusb) as a `BulkLink` of the TypeScript
// protocol client (packages/dk-session/src/bulk.ts), shaped like midi-transport.ts. The native side moves whole frames and starts no process.
import { SysExReplyLink, type BulkCandidate, type BulkLink } from '@digi/dk-session';
import type { BulkHost } from './host';

export class TauriBulkLink implements BulkLink {
  private constructor(private readonly bulk: BulkHost) {}

  /** The devices on the bus that offer the bulk interface (usb-fast loaded): `id` is `bus:address`. Their USB serial strings are the same: the nonce handshake tells which is ours. */
  static candidates = async (bulk: BulkHost): Promise<BulkCandidate[]> =>
    (await bulk.list()).filter((c) => !c.out_only).map(({ id, name }) => ({ id, name }));
  /** The devices with an OUT-only interface (USB AUDIO/MIDI mode): replies come over SysEx. */
  static outOnlyCandidates = async (bulk: BulkHost): Promise<BulkCandidate[]> =>
    (await bulk.list()).filter((c) => c.out_only).map(({ id, name }) => ({ id, name }));
  /** Claim the OUT-only interface of the device `id`; the replies come from `poll` (the SysEx 'R' poll). Closing the link releases the interface. */
  static async openOutOnly(bulk: BulkHost, id: string, poll: () => Promise<Uint8Array>): Promise<BulkLink> {
    await bulk.openOutOnly(id);
    return new SysExReplyLink(
      (frame) => bulk.sendOutOnly(frame),
      poll,
      () => bulk.closeOutOnly(),
    );
  }
  /** Claim the device `id`; the interface of a link opened before is released first. */
  static async open(bulk: BulkHost, id: string): Promise<TauriBulkLink> {
    await bulk.open(id);
    return new TauriBulkLink(bulk);
  }

  async send(frame: Uint8Array): Promise<void> {
    await this.bulk.send(frame);
  }

  /** The next reply frame, or null after `timeoutMs` or when `signal` aborts (the native wait ends too). */
  async recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (signal?.aborted) return null;
    const onAbort = () => void this.bulk.cancel().catch(() => {});
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      const got = await this.bulk.recv(timeoutMs);
      return got.byteLength ? new Uint8Array(got) : null;
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
  }

  async close(): Promise<void> {
    await this.bulk.close();
  }
}

// The USB bulk link of the desktop app: the Tauri commands of `bulk_port.rs` (nusb) as a `BulkLink` of the TypeScript protocol client
// (packages/dk-session/src/bulk.ts), shaped like midi-transport.ts. The native side moves whole frames and starts no process.
import { invoke } from '@tauri-apps/api/core';
import { SysExReplyLink, type BulkCandidate, type BulkLink } from '@digi/dk-session';

/** What `bulk_list` returns: `out_only` marks the OUT-only interface (USB AUDIO/MIDI mode). */
type ListedCandidate = BulkCandidate & { out_only: boolean };
const listed = () => invoke<ListedCandidate[]>('bulk_list');

/** End the pending `recv` on the native side now (it answers as if its time had run out). */
const cancelBulkRecv = () => invoke<void>('bulk_cancel');

export class TauriBulkLink implements BulkLink {
  /** The devices on the bus that offer the bulk interface (usb-fast loaded): `id` is `bus:address`. Their USB serial strings are the same: the nonce handshake tells which is ours. */
  static candidates = async (): Promise<BulkCandidate[]> =>
    (await listed()).filter((c) => !c.out_only).map(({ id, name }) => ({ id, name }));
  /** The devices with an OUT-only interface (USB AUDIO/MIDI mode): replies come over SysEx. */
  static outOnlyCandidates = async (): Promise<BulkCandidate[]> =>
    (await listed()).filter((c) => c.out_only).map(({ id, name }) => ({ id, name }));
  /** Claim the OUT-only interface of the device `id`; the replies come from `poll` (the SysEx 'R' poll). Closing the link releases the interface. */
  static async openOutOnly(id: string, poll: () => Promise<Uint8Array>): Promise<BulkLink> {
    await invoke<void>('bulk_open_out_only', { id });
    return new SysExReplyLink(
      (frame) => invoke<void>('bulk_send_out_only', frame),
      poll,
      () => invoke<void>('bulk_close_out_only'),
    );
  }
  /** Claim the device `id`; the interface of a link opened before is released first. */
  static async open(id: string): Promise<TauriBulkLink> {
    await invoke<void>('bulk_open', { id });
    return new TauriBulkLink();
  }

  async send(frame: Uint8Array): Promise<void> {
    await invoke('bulk_send', frame);
  }

  /** The next reply frame, or null after `timeoutMs` or when `signal` aborts (the native wait ends too). */
  async recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (signal?.aborted) return null;
    const onAbort = () => void cancelBulkRecv().catch(() => {});
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      const got = await invoke<ArrayBuffer>('bulk_recv', { timeoutMs: Math.max(1, Math.round(timeoutMs)) });
      return got.byteLength ? new Uint8Array(got) : null;
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
  }

  async close(): Promise<void> {
    await invoke<void>('bulk_close');
  }
}

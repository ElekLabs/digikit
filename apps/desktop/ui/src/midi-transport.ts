// The SysEx path of a real device: a MIDI port the app opens through the host's `ports.midi` (`midi_port.rs`, midir), as a transport of the
// TypeScript protocol client (`DkSession`), shaped like desktop-transport.ts. The native side moves whole messages and starts no process.
import type { DkTransport } from '@digi/dk-session';
import type { MidiHost } from './host/host';

export class MidiPortTransport implements DkTransport {
  constructor(private readonly midi: MidiHost) {}

  async send(msg: Uint8Array): Promise<void> {
    await this.midi.send(msg);
  }

  /** The next complete message the device sent, or null after `timeoutMs` or when `signal` aborts (the native wait ends too). */
  async recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (signal?.aborted) return null;
    const onAbort = () => void this.midi.cancel().catch(() => {});
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      const got = await this.midi.recv(timeoutMs);
      return got.byteLength ? new Uint8Array(got) : null;
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
  }
}

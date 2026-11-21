// The SysEx path of a real device: a MIDI port the app opens through the host's `ports.midi` (`midi_port.rs`, midir), as a transport of the
// TypeScript protocol client (`DkSession`), shaped like desktop-transport.ts. The native side moves whole messages and starts no process.
// The transport owns the link: it opens the port, opens it again when the device re-enumerated (`recover`) and closes it.
import type { DkTransport } from '@digi/dk-session';
import type { MidiHost } from './host';

export class MidiPortTransport implements DkTransport {
  /** `port` is the name of a port that is already open on `midi` (`open` does both). */
  constructor(
    private readonly midi: MidiHost,
    private readonly port: string,
  ) {}

  /** Open the named port and return the transport on it. */
  static async open(midi: MidiHost, port: string): Promise<MidiPortTransport> {
    await midi.open(port);
    return new MidiPortTransport(midi, port);
  }

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

  /** Open the same port again: the handle goes stale when the device restarts and re-enumerates. The native open keeps the old connection until the new one is open. */
  async recover(): Promise<void> {
    try {
      await this.midi.open(this.port);
    } catch {
      // the device is not back yet; the next failed exchange calls recover again
    }
  }

  async close(): Promise<void> {
    await this.midi.close();
  }
}

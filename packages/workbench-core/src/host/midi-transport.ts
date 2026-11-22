// The SysEx path of a real device: a MIDI port the app opens through the host's `ports.midi` (`midi_port.rs`, midir), as a transport of the
// TypeScript protocol client (`DkSession`), shaped like desktop-transport.ts. The native side moves whole messages and starts no process.
// The transport owns the link: it opens the port, opens it again when the device re-enumerated (`recover`) and closes it.
import type { DkTransport } from '@digi/dk-session';
import type { MidiHost } from './host';

export class MidiPortTransport implements DkTransport {
  private closing?: Promise<void>;
  // Native commands run on worker threads against one shared port slot.
  private readonly writes = new Set<Promise<void>>();
  private readonly reads = new Set<Promise<ArrayBuffer>>();

  private async track<T>(pending: Set<Promise<T>>, command: Promise<T>): Promise<T> {
    pending.add(command);
    try {
      return await command;
    } finally {
      pending.delete(command);
    }
  }

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
    if (this.closing) throw new Error('The MIDI port is closed.');
    await this.track(this.writes, this.midi.send(msg));
  }

  /** The next complete message the device sent, or null after `timeoutMs` or when `signal` aborts (the native wait ends too). */
  async recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (this.closing || signal?.aborted) return null;
    const onAbort = () => {
      if (!this.closing) void this.track(this.writes, this.midi.cancel()).catch(() => {});
    };
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      const got = await this.track(this.reads, this.midi.recv(timeoutMs));
      return got.byteLength ? new Uint8Array(got) : null;
    } finally {
      signal?.removeEventListener('abort', onAbort);
    }
  }

  /** Open the same port again: the handle goes stale when the device restarts and re-enumerates. The native open keeps the old connection until the new one is open. */
  async recover(): Promise<void> {
    if (this.closing) return;
    try {
      await this.track(this.writes, this.midi.open(this.port));
    } catch {
      // the device is not back yet; the next failed exchange calls recover again
    }
  }

  async drain(): Promise<void> {
    await Promise.allSettled([...this.reads, ...this.writes]);
  }

  close(): Promise<void> {
    return (this.closing ??= this.drainAndClose());
  }

  private async drainAndClose(): Promise<void> {
    await Promise.allSettled(this.writes);
    await this.midi.close();
    // Closing wakes a native receive. Finish it before the host reuses the slot.
    await Promise.allSettled(this.reads);
  }
}

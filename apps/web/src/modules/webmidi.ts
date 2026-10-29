// Web MIDI (with SysEx permission) as a DkTransport. The access object is passed in, so the tests give it a mock.
// Written without TS-only runtime syntax so node can import it.

import type { DkTransport } from '@digi/dk-session';

/** The parts of the Web MIDI API this file uses. */
export interface MidiPortLike {
  id: string;
  name?: string | null;
  manufacturer?: string | null;
  state?: string;
}
export interface MidiInputLike extends MidiPortLike {
  onmidimessage: ((e: { data: Uint8Array | null }) => void) | null;
  open?(): Promise<unknown>;
}
export interface MidiOutputLike extends MidiPortLike {
  send(data: Uint8Array | number[]): void;
  open?(): Promise<unknown>;
}
export interface MidiAccessLike {
  /** Called when a port appears, disappears or changes state (`port.state` is `disconnected` for a port that is gone). */
  onstatechange?: ((e: { port: MidiPortLike | null }) => void) | null;
  inputs: Iterable<[string, MidiInputLike]> | { values(): Iterable<MidiInputLike> };
  outputs: Iterable<[string, MidiOutputLike]> | { values(): Iterable<MidiOutputLike> };
}

const values = <T>(m: { values(): Iterable<T> } | Iterable<[string, T]>): T[] =>
  'values' in m && typeof m.values === 'function'
    ? Array.from(m.values())
    : Array.from(m as Iterable<[string, T]>, (e) => e[1]);

export const portLabel = (p: MidiPortLike) => [p.manufacturer, p.name].filter(Boolean).join(' ') || p.id;

export const NO_WEBMIDI = 'This browser has no Web MIDI. Use Chrome or Edge, or the desktop app.';
export const SYSEX_DENIED =
  'Web MIDI with SysEx was not allowed. Allow MIDI access for this site (the lock icon in the address bar), then reload the page.';

/** The Web MIDI access with SysEx permission, or an error with a message the user can act on. */
export async function requestAccess(): Promise<MidiAccessLike> {
  const nav = globalThis.navigator as unknown as {
    requestMIDIAccess?: (o: { sysex: boolean }) => Promise<MidiAccessLike>;
  };
  if (!nav?.requestMIDIAccess) throw new Error(NO_WEBMIDI);
  try {
    return await nav.requestMIDIAccess({ sysex: true });
  } catch (e) {
    const name = (e as { name?: string } | null)?.name;
    if (name === 'SecurityError' || name === 'NotAllowedError') throw new Error(SYSEX_DENIED);
    throw new Error(`Web MIDI could not start: ${e instanceof Error ? e.message : String(e)}`);
  }
}

export function listPorts(access: MidiAccessLike): { inputs: MidiInputLike[]; outputs: MidiOutputLike[] } {
  return { inputs: values(access.inputs), outputs: values(access.outputs) };
}

const disconnected = (label: string) =>
  new Error(`The MIDI port ${label} was disconnected. Connect the device again and pick the port.`);

/**
 * One input and one output. SysEx arrives as one event or in pieces; a message is complete at F7. Real-time bytes
 * (F8..FF) inside it are dropped. With `access`, a port that goes away (`onstatechange`) ends the transport: a pending `recv`
 * and every later `send` or `recv` fail with a clear message, and `onLost` is called once.
 */
export class WebMidiTransport implements DkTransport {
  private input: MidiInputLike;
  private output: MidiOutputLike;
  private access: MidiAccessLike | undefined;
  private buf: number[] | null = null;
  private queue: Uint8Array[] = [];
  private waiter: { resolve: (m: Uint8Array | null) => void; reject: (e: Error) => void } | null = null;
  private lost: Error | null = null;
  private onLost: ((e: Error) => void) | undefined;
  private previousState: MidiAccessLike['onstatechange'];

  constructor(input: MidiInputLike, output: MidiOutputLike, access?: MidiAccessLike, onLost?: (e: Error) => void) {
    this.input = input;
    this.output = output;
    this.access = access;
    this.onLost = onLost;
    input.onmidimessage = (e) => this.feed(e.data);
    if (access) {
      this.previousState = access.onstatechange;
      access.onstatechange = (e) => {
        this.previousState?.(e);
        const p = e.port;
        if (p && (p.id === input.id || p.id === output.id) && p.state === 'disconnected') this.fail(p.name ?? p.id);
      };
    }
  }
  static async open(
    input: MidiInputLike,
    output: MidiOutputLike,
    access?: MidiAccessLike,
    onLost?: (e: Error) => void,
  ): Promise<WebMidiTransport> {
    await input.open?.();
    await output.open?.();
    return new WebMidiTransport(input, output, access, onLost);
  }

  private fail(label: string) {
    if (this.lost) return;
    this.lost = disconnected(`"${label}"`);
    const w = this.waiter;
    this.waiter = null;
    w?.reject(this.lost);
    this.onLost?.(this.lost);
  }

  private feed(data: Uint8Array | null) {
    for (const b of data ?? []) {
      if (b >= 0xf8) continue;
      if (b === 0xf0) this.buf = [b];
      else if (this.buf) {
        this.buf.push(b);
        if (b === 0xf7) {
          const m = Uint8Array.from(this.buf);
          this.buf = null;
          if (this.waiter) {
            const w = this.waiter;
            this.waiter = null;
            w.resolve(m);
          } else this.queue.push(m);
        } else if (b >= 0x80) this.buf = null; // another status byte: not a SysEx message any more
      }
    }
  }

  send(msg: Uint8Array): void {
    if (this.lost) throw this.lost;
    this.output.send(msg);
  }
  /** The next message, null after `timeoutMs` or when `signal` aborts (the wait ends at once); fails when the port is gone. */
  recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (this.lost) return Promise.reject(this.lost);
    const q = this.queue.shift();
    if (q) return Promise.resolve(q);
    if (signal?.aborted) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const done = () => {
        clearTimeout(timer);
        signal?.removeEventListener('abort', onAbort);
        if (this.waiter === me) this.waiter = null;
      };
      const me = {
        resolve: (m: Uint8Array | null) => {
          done();
          resolve(m);
        },
        reject: (e: Error) => {
          done();
          reject(e);
        },
      };
      const timer = setTimeout(() => me.resolve(null), timeoutMs);
      const onAbort = () => me.resolve(null);
      signal?.addEventListener('abort', onAbort, { once: true });
      this.waiter = me;
    });
  }
  close() {
    this.input.onmidimessage = null;
    if (this.access) this.access.onstatechange = this.previousState ?? null;
  }
}

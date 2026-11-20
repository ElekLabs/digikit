// MIDI to a target: notes, control changes, NRPN and transport bytes on the port the SysEx session already uses, for the calibration runner
// and the tests. dk-session carries SysEx only; this is the other half of what a host sends. It stays below the apps: the target is a
// `MidiSink` (the host's MIDI port, or the emulator's `midi.usb.in`), so a test gives a fake.
//
// No interleaving with SysEx: every `send` of a sink moves one whole message (the desktop's `midi_send` holds the port lock for the whole
// write; the emulator's `midi.usb.in` appends the bytes of one call to a FIFO in one step), and the session's `MidiPortTransport` and
// `EmuUsbTransport` send each SysEx message in one call. So a note lands before or after a SysEx message, never inside one. A `MidiOut`
// also keeps its own messages in call order (an NRPN is four control changes that nothing else of the same `MidiOut` separates).
import type { EmuRpcCall } from './emu-usb-transport.ts';

/** Where MIDI bytes go: one call is one whole message. The workbench host's `ports.midi` is one (its extra members are not used). */
export interface MidiSink {
  send(bytes: Uint8Array): Promise<void>;
}

/** The firmware's own USB MIDI receive path in the emulator (`midi.usb.in`). Throws when the controller model is wedged and drops the input. */
export function emuMidiSink(call: EmuRpcCall): MidiSink {
  return {
    async send(bytes) {
      const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
      const sent = await call<{ queued: number }>('midi.usb.in', { bytes: hex });
      if (bytes.length > 0 && sent.queued === 0)
        throw new Error('USB MIDI input dropped (the USB controller model is wedged)');
    },
  };
}

function check(what: string, v: number, lo: number, hi: number): number {
  if (!Number.isInteger(v) || v < lo || v > hi) throw new RangeError(`MIDI ${what} ${v} is not ${lo} to ${hi}`);
  return v;
}
const channel = (ch: number) => check('channel', ch, 1, 16) - 1;
const data = (what: string, v: number) => check(what, v, 0, 127);

/** Channel messages and NRPN on a sink. `ch` is 1 to 16. Use one `MidiOut` per target. */
export class MidiOut {
  private readonly sink: MidiSink;

  constructor(sink: MidiSink) {
    this.sink = sink;
  }

  private tail: Promise<void> = Promise.resolve();

  /** Send `messages` (each one whole MIDI message) in order, after everything sent before; a failure fails this call only. */
  private enqueue(messages: number[][]): Promise<void> {
    const run = async () => {
      for (const m of messages) await this.sink.send(Uint8Array.from(m));
    };
    const done = this.tail.then(run);
    this.tail = done.catch(() => {});
    return done;
  }

  /** Raw bytes of one whole message. */
  send(bytes: Uint8Array | number[]): Promise<void> {
    return this.enqueue([Array.from(bytes)]);
  }

  async note(ch: number, key: number, velocity: number): Promise<void> {
    await this.enqueue([[0x90 | channel(ch), data('note', key), check('velocity', velocity, 1, 127)]]);
  }

  async noteOff(ch: number, key: number): Promise<void> {
    await this.enqueue([[0x80 | channel(ch), data('note', key), 0]]);
  }

  /** Control change; `value` 0 to 127. */
  async cc(ch: number, num: number, value: number): Promise<void> {
    await this.enqueue([[0xb0 | channel(ch), data('controller', num), data('value', value)]]);
  }

  /** NRPN `msb`/`lsb` set to the 14-bit `value`: CC 99, 98, then data entry CC 6 (high 7 bits) and CC 38 (low 7 bits). */
  async nrpn(ch: number, msb: number, lsb: number, value14: number): Promise<void> {
    const c = channel(ch);
    const v = check('NRPN value', value14, 0, 0x3fff);
    await this.enqueue([
      [0xb0 | c, 99, data('NRPN msb', msb)],
      [0xb0 | c, 98, data('NRPN lsb', lsb)],
      [0xb0 | c, 6, v >> 7],
      [0xb0 | c, 38, v & 0x7f],
    ]);
  }

  /** MIDI start (0xFA). */
  start(): Promise<void> {
    return this.enqueue([[0xfa]]);
  }

  /** MIDI stop (0xFC). */
  stop(): Promise<void> {
    return this.enqueue([[0xfc]]);
  }
}

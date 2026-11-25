import { emuMidiSink, MidiOut } from '@digi/dk-session';
import { pollUntil } from '@digi/test-support';
import { rpc } from './guest-access.ts';
import type { Rt } from './panel-waits.ts';

/**
 * The firmware's own MIDI CC path: the bytes go to `midi.usb.in` and reach the firmware's USB MIDI receive callback, as if a host had sent them.
 * A CC value v is stored by the firmware as `v << 8` in the parameter word.
 * The call returns when the firmware has taken the bytes: `queued` is the length of the input queue, and a wedged USB controller model drops the input and reports 0.
 */
export async function sendMidi(rt: Rt, bytes: number[]): Promise<void> {
  await midiOut(rt).send(bytes);
}

/** The emulator's `midi.usb.in` as a `MidiOut`; every message waits until the firmware has taken it. */
function midiOut(rt: Rt): MidiOut {
  const emu = emuMidiSink((method, params) => rpc(rt.session, method, params));
  return new MidiOut({
    async send(bytes) {
      await emu.send(bytes);
      // empty bytes push nothing and return the current queue length
      await pollUntil(
        rt.clock,
        () => rpc<{ queued: number }>(rt.session, 'midi.usb.in', { bytes: '' }),
        (got) => got.queued === 0,
        {
          withinMs: 1000,
          intervalMs: 20,
          message: () => 'USB MIDI input not taken by the firmware',
        },
      );
    },
  });
}

/** A control change on `channel` (1 to 16), controller `cc` and `value` (0 to 127). */
export async function sendCc(rt: Rt, channel: number, cc: number, value: number): Promise<void> {
  await midiOut(rt).cc(channel, cc, value);
}

/** A note on `channel` (1 to 16), `note` and `velocity` (1 to 127). */
export async function sendNote(rt: Rt, channel: number, note: number, velocity: number): Promise<void> {
  await midiOut(rt).note(channel, note, velocity);
}

/** A note off on `channel` (1 to 16) for `note`. */
export async function sendNoteOff(rt: Rt, channel: number, note: number): Promise<void> {
  await midiOut(rt).noteOff(channel, note);
}

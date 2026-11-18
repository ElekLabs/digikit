import { pollUntil } from '@digi/test-support';
import { rpc, type Rt } from './fx-live.ts';

/**
 * The firmware's own MIDI CC path: the bytes go to `midi.usb.in` and reach the firmware's USB MIDI receive callback, as if a host had sent them.
 * A CC value v is stored by the firmware as `v << 8` in the parameter word.
 * The call returns when the firmware has taken the bytes: `queued` is the length of the input queue, and a wedged USB controller model drops the input and reports 0.
 */
export async function sendMidi(rt: Rt, bytes: number[]): Promise<void> {
  const hex = bytes.map((b) => b.toString(16).padStart(2, '0')).join('');
  const sent = await rpc<{ queued: number }>(rt.session, 'midi.usb.in', { bytes: hex });
  if (bytes.length > 0 && sent.queued === 0)
    throw new Error('USB MIDI input dropped (the USB controller model is wedged)');
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
}

/** A control change on `channel` (1 to 16), controller `cc` and `value` (0 to 127). */
export async function sendCc(rt: Rt, channel: number, cc: number, value: number): Promise<void> {
  await sendMidi(rt, [0xb0 | (channel - 1), cc, value]);
}

/** A note on `channel` (1 to 16), `note` and `velocity` (1 to 127). */
export async function sendNote(rt: Rt, channel: number, note: number, velocity: number): Promise<void> {
  await sendMidi(rt, [0x90 | (channel - 1), note, velocity]);
}

/** A note off on `channel` (1 to 16) for `note`. */
export async function sendNoteOff(rt: Rt, channel: number, note: number): Promise<void> {
  await sendMidi(rt, [0x80 | (channel - 1), note, 0]);
}

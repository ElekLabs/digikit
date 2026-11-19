import { describe, expect, test } from 'vitest';
import { MidiOut } from '@digi/dk-session';
import { createFakeHost } from '../src/host/fake-host';
import { MidiPortTransport } from '../src/host/midi-transport';

describe('MIDI to a device through the host port', () => {
  test('notes and the session transport share the port, one whole message per send', async () => {
    const host = createFakeHost();
    const out = new MidiOut(host.ports.midi);
    const transport = new MidiPortTransport(host.ports.midi);
    const sysex = Uint8Array.of(0xf0, 0x00, 0x20, 0x3c, 1, 2, 3, 0xf7);
    await Promise.all([out.note(1, 60, 100), transport.send(sysex), out.cc(1, 74, 5)]);
    const sent = host.argsOf('ports.midi.send').map(([b]) => Array.from(b as Uint8Array));
    // each send is a complete message: the SysEx is never cut by a channel message
    expect(sent).toContainEqual(Array.from(sysex));
    for (const m of sent) {
      if (m[0] === 0xf0) expect(m.at(-1)).toBe(0xf7);
      else expect(m).toHaveLength(3);
    }
    expect(sent).toContainEqual([0x90, 60, 100]);
    expect(sent).toContainEqual([0xb0, 74, 5]);
  });
});

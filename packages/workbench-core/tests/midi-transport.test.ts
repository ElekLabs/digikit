// The MIDI port transport owns its link: it opens the named port, opens the same port again on recover, and closes it.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createFakeHost } from '../src/host/fake-host.ts';
import { MidiPortTransport } from '../src/host/midi-transport.ts';

test('open opens the named port', async () => {
  const host = createFakeHost();
  await MidiPortTransport.open(host.ports.midi, 'Elektron Digitakt II');
  assert.deepEqual(host.argsOf('ports.midi.open'), [['Elektron Digitakt II']]);
});

test('recover opens the same port again and swallows a failure', async () => {
  const host = createFakeHost();
  const transport = await MidiPortTransport.open(host.ports.midi, 'Elektron Digitone II');
  await transport.recover();
  assert.deepEqual(host.argsOf('ports.midi.open'), [['Elektron Digitone II'], ['Elektron Digitone II']]);
  host.replies['ports.midi.open'] = () => {
    throw new Error('The MIDI port is not there.');
  };
  await transport.recover();
  assert.equal(host.argsOf('ports.midi.open').length, 3);
});

test('close closes the port', async () => {
  const host = createFakeHost();
  const transport = await MidiPortTransport.open(host.ports.midi, 'Elektron Digitakt II');
  await transport.close();
  assert.deepEqual(host.commands(), ['ports.midi.open', 'ports.midi.close']);
});

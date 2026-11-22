// The MIDI port transport owns its link: it opens the named port, opens the same port again on recover, and closes it.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createFakeHost } from '../src/host/fake-host.ts';
import { MidiPortTransport } from '../src/host/midi-transport.ts';

test('open opens the named port', async () => {
  const host = createFakeHost();
  await MidiPortTransport.open(host.ports.midi, 'Port A');
  assert.deepEqual(host.argsOf('ports.midi.open'), [['Port A']]);
});

test('recover opens the same port again and swallows a failure', async () => {
  const host = createFakeHost();
  const transport = await MidiPortTransport.open(host.ports.midi, 'Port B');
  await transport.recover();
  assert.deepEqual(host.argsOf('ports.midi.open'), [['Port B'], ['Port B']]);
  host.replies['ports.midi.open'] = () => {
    throw new Error('The MIDI port is not there.');
  };
  await transport.recover();
  assert.equal(host.argsOf('ports.midi.open').length, 3);
});

test('recover after close does not open the port again', async () => {
  const host = createFakeHost();
  const transport = await MidiPortTransport.open(host.ports.midi, 'Port A');
  await transport.close();
  await transport.recover();
  assert.deepEqual(host.argsOf('ports.midi.open'), [['Port A']]);
});

test('close closes the port', async () => {
  const host = createFakeHost();
  const transport = await MidiPortTransport.open(host.ports.midi, 'Port A');
  await transport.close();
  assert.deepEqual(host.commands(), ['ports.midi.open', 'ports.midi.close']);
});

test('a released transport cannot send, receive or close the replacement port', async () => {
  const host = createFakeHost();
  const a = await MidiPortTransport.open(host.ports.midi, 'A');
  await a.close();
  await MidiPortTransport.open(host.ports.midi, 'B');
  await assert.rejects(a.send(Uint8Array.of(1)), /closed/);
  assert.equal(await a.recv(5), null);
  await a.close();
  assert.deepEqual(host.commands(), ['ports.midi.open', 'ports.midi.close', 'ports.midi.open']);
});

test('close drains an outstanding native port recovery before releasing the MIDI slot', async () => {
  const host = createFakeHost();
  const a = await MidiPortTransport.open(host.ports.midi, 'A');
  let finish!: () => void;
  host.replies['ports.midi.open'] = () => new Promise<void>((resolve) => (finish = resolve));
  const recovery = a.recover();
  const closing = a.close();
  await Promise.resolve();
  assert.equal(host.argsOf('ports.midi.close').length, 0);
  finish();
  await Promise.all([recovery, closing]);
  assert.equal(host.argsOf('ports.midi.close').length, 1);
});

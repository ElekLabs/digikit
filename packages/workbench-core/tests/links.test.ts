// The USB bulk link and the MIDI transport of the desktop app over a FakeHost: the order of their host calls, and abort.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { TauriBulkLink } from '../src/host/bulk-link.ts';
import { createFakeHost } from '../src/host/fake-host.ts';
import { MidiPortTransport } from '../src/host/midi-transport.ts';

const frame = (...b: number[]) => new Uint8Array(b);

test('a bulk link opens, sends, receives and closes in that order', async () => {
  const host = createFakeHost({ 'ports.bulk.recv': new Uint8Array([9, 8]).buffer });
  const link = await TauriBulkLink.open(host.ports.bulk, '1:4');
  await link.send(frame(1, 2));
  assert.deepEqual(await link.recv(250), new Uint8Array([9, 8]));
  await link.close();
  assert.deepEqual(host.commands(), ['ports.bulk.open', 'ports.bulk.send', 'ports.bulk.recv', 'ports.bulk.close']);
  assert.deepEqual(host.argsOf('ports.bulk.open'), [['1:4']]);
});

test('an empty bulk reply is a timeout', async () => {
  const host = createFakeHost({ 'ports.bulk.recv': new ArrayBuffer(0) });
  const link = await TauriBulkLink.open(host.ports.bulk, '1:4');
  assert.equal(await link.recv(10), null);
});

test('candidates split the listing by interface kind', async () => {
  const host = createFakeHost({
    'ports.bulk.list': [
      { id: '1:4', name: 'DT2', out_only: false },
      { id: '1:5', name: 'DN2', out_only: true },
    ],
  });
  assert.deepEqual(await TauriBulkLink.candidates(host.ports.bulk), [{ id: '1:4', name: 'DT2' }]);
  assert.deepEqual(await TauriBulkLink.outOnlyCandidates(host.ports.bulk), [{ id: '1:5', name: 'DN2' }]);
});

test('the OUT-only link sends over its own commands and releases the interface on close', async () => {
  const host = createFakeHost();
  const link = await TauriBulkLink.openOutOnly(host.ports.bulk, '1:5', async () => frame());
  await link.send(frame(7));
  await link.close();
  assert.deepEqual(host.commands(), ['ports.bulk.openOutOnly', 'ports.bulk.sendOutOnly', 'ports.bulk.closeOutOnly']);
});

test('an abort during a pending receive cancels the native wait', async () => {
  const host = createFakeHost({ 'ports.bulk.recv': () => new Promise(() => {}) });
  const link = await TauriBulkLink.open(host.ports.bulk, '1:4');
  const abort = new AbortController();
  void link.recv(5000, abort.signal);
  abort.abort();
  assert.ok(host.commands().includes('ports.bulk.cancel'));
});

test('an already aborted receive asks nothing of the host', async () => {
  const host = createFakeHost();
  const abort = new AbortController();
  abort.abort();
  assert.equal(await new MidiPortTransport(host.ports.midi).recv(100, abort.signal), null);
  assert.deepEqual(host.calls, []);
});

test('a MIDI transport sends and receives through the open port', async () => {
  const host = createFakeHost({ 'ports.midi.recv': new Uint8Array([0xf0, 0xf7]).buffer });
  await host.ports.midi.open('Elektron Digitakt II');
  const transport = new MidiPortTransport(host.ports.midi);
  await transport.send(frame(0xf0, 1, 0xf7));
  assert.deepEqual(await transport.recv(100), new Uint8Array([0xf0, 0xf7]));
  await host.ports.midi.close();
  assert.deepEqual(host.commands(), ['ports.midi.open', 'ports.midi.send', 'ports.midi.recv', 'ports.midi.close']);
});

// The connection indicator of the Transfer pane (apps/web/src/transfer/connection-core.ts): what moves it between connected, connecting and
// not connected, and the text it shows.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError } from '@digi/dk-session';
import {
  CONNECTING,
  connectionView,
  isLost,
  parseSoftwareVersion,
  lostReason,
  shortId,
  step,
  type Connection,
  type DeviceFacts,
} from '../src/transfer/connection-core.ts';

const DEVICE: DeviceFacts = { id: 'dt2', name: 'Digitakt II', runtime: '0.5.2' };
const connected: Connection = { state: 'connected', device: DEVICE };

test('an answered INFO is connected; a request on the way is connecting until then', () => {
  assert.deepEqual(step(CONNECTING, { type: 'answered', device: DEVICE }), connected);
  assert.deepEqual(step({ state: 'disconnected', reason: 'x' }, { type: 'asking' }), CONNECTING);
  assert.deepEqual(step(CONNECTING, { type: 'asking' }), CONNECTING);
});

test('a read while connected does not flicker the indicator', () => {
  assert.equal(step(connected, { type: 'asking' }), connected);
});

test('a lost request, a closed port and a new target move it', () => {
  assert.deepEqual(step(connected, { type: 'lost', reason: 'no reply' }), {
    state: 'disconnected',
    reason: 'no reply',
  });
  assert.equal(step(connected, { type: 'closed' }).state, 'disconnected');
  assert.deepEqual(step(connected, { type: 'target' }), CONNECTING);
});

test('a cancel ends "connecting", and leaves a device that had answered', () => {
  assert.equal(step(CONNECTING, { type: 'cancelled' }).state, 'disconnected');
  assert.equal(step(connected, { type: 'cancelled' }), connected);
});

test('a device with no runtime (the stock firmware, on the Firmware page) shows its OS and says so', () => {
  const stock: Connection = { state: 'connected', device: { id: 'dn2', name: 'Digitone II', os: '1.12' } };
  const v = connectionView(stock, 'MIDI: Elektron Digitone II');
  assert.deepEqual(v.parts, ['Digitone II', 'OS 1.12', 'no runtime', 'MIDI: Elektron Digitone II']);
  assert.match(v.title, /no runtime/);
});

test('no reply, a closed port and a paused emulator are lost; a device error and a cancel are not', () => {
  assert.equal(isLost(new DkError({ error: 'no reply from the device', kind: 'timeout' })), true);
  assert.equal(isLost(new Error('The MIDI port is closed')), true);
  assert.equal(isLost('midi_send: no output'), true);
  assert.equal(isLost(new DkError({ error: 'E_FULL', kind: 'device', status: 'E_FULL' })), false);
  assert.equal(isLost(new DkError({ error: 'bad reply', kind: 'protocol' })), false);
  assert.equal(isLost(new DkError({ error: 'cancelled', kind: 'cancelled' })), false);
  assert.equal(lostReason(new Error('first line\nsecond')), 'first line');
});

test('a long id is cut, the tooltip keeps all of it', () => {
  assert.equal(shortId('dt2'), 'dt2');
  assert.equal(shortId('0123456789abcdef'), '01234567…');
  const v = connectionView({ state: 'connected', device: { ...DEVICE, id: '0123456789abcdef' } }, 'MIDI: Port');
  assert.ok(v.parts.includes('id 01234567…'));
  assert.match(v.title, /id 0123456789abcdef/);
});

test('connected shows the device, the runtime, the id and the port; the OS only when it is known', () => {
  const v = connectionView(connected, 'MIDI: Elektron Digitakt II');
  assert.equal(v.label, 'Connected');
  assert.deepEqual(v.parts, ['Digitakt II', 'runtime 0.5.2', 'id dt2', 'MIDI: Elektron Digitakt II']);
  const withOs = connectionView({ state: 'connected', device: { ...DEVICE, os: '1.17' } }, 'Emulator');
  assert.deepEqual(withOs.parts.slice(0, 3), ['Digitakt II', 'OS 1.17', 'runtime 0.5.2']);
});

test('connecting and not connected show the port and, for the second, why', () => {
  assert.deepEqual(connectionView(CONNECTING, 'Emulator'), {
    state: 'connecting',
    label: 'Connecting…',
    parts: ['Emulator'],
    title: 'Asking Emulator',
  });
  const lost = connectionView({ state: 'disconnected', reason: 'no reply from the device' }, 'MIDI: X');
  assert.equal(lost.label, 'Not connected');
  assert.equal(lost.title, 'MIDI: X: no reply from the device');
});

test('the OS version is the version-like string of the SoftwareVersion reply', () => {
  const reply = (...parts: string[]) => new TextEncoder().encode(parts.join('\0') + '\0');
  assert.equal(parseSoftwareVersion(reply('00', '79', '1.17')), '1.17');
  assert.equal(parseSoftwareVersion(reply('1.15C', '79')), '1.15C');
  assert.equal(parseSoftwareVersion(reply('00', '79')), undefined);
  assert.equal(parseSoftwareVersion(new Uint8Array()), undefined);
});

test('a port that went away says why in the indicator', () => {
  assert.deepEqual(step(connected, { type: 'closed', reason: 'The MIDI port "X" was disconnected.' }), {
    state: 'disconnected',
    reason: 'The MIDI port "X" was disconnected.',
  });
  assert.equal((step(connected, { type: 'closed' }) as { reason: string }).reason, 'The MIDI port is gone.');
});

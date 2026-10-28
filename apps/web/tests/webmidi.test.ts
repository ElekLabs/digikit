// The Web MIDI transport (src/modules/webmidi.ts) over a fake MIDIAccess: cancel, split SysEx, a port that goes away, no Web MIDI, no permission.
import assert from 'node:assert/strict';
import { afterEach, test } from 'vitest';
import {
  NO_WEBMIDI,
  SYSEX_DENIED,
  WebMidiTransport,
  requestAccess,
  type MidiAccessLike,
  type MidiInputLike,
  type MidiOutputLike,
} from '../src/modules/webmidi.ts';

function fake() {
  const sent: number[][] = [];
  const input: MidiInputLike = { id: 'in1', name: 'Elektron Digitakt II', state: 'connected', onmidimessage: null };
  const output: MidiOutputLike = {
    id: 'out1',
    name: 'Elektron Digitakt II',
    state: 'connected',
    send: (d) => void sent.push(Array.from(d)),
  };
  const access: MidiAccessLike = { inputs: [['in1', input]], outputs: [['out1', output]], onstatechange: null };
  const push = (...bytes: number[]) => input.onmidimessage?.({ data: Uint8Array.from(bytes) });
  return { sent, input, output, access, push };
}

test('a SysEx message in pieces is one message; real-time bytes inside it are dropped', async () => {
  const f = fake();
  const t = await WebMidiTransport.open(f.input, f.output, f.access);
  const next = t.recv(1000);
  f.push(0xf0, 0x7d, 1);
  f.push(0xf8, 2, 3);
  f.push(0xf7);
  assert.deepEqual(Array.from((await next)!), [0xf0, 0x7d, 1, 2, 3, 0xf7]);
});

test('a message that arrives before recv is queued', async () => {
  const f = fake();
  const t = await WebMidiTransport.open(f.input, f.output, f.access);
  f.push(0xf0, 1, 0xf7);
  assert.deepEqual(Array.from((await t.recv(1000))!), [0xf0, 1, 0xf7]);
});

test('an abort ends a pending recv at once with null, and a later message is not lost', async () => {
  const f = fake();
  const t = await WebMidiTransport.open(f.input, f.output, f.access);
  const ac = new AbortController();
  const started = Date.now();
  const pending = t.recv(60_000, ac.signal);
  ac.abort();
  assert.equal(await pending, null);
  assert.ok(Date.now() - started < 1000);
  const after = t.recv(1000);
  f.push(0xf0, 9, 0xf7);
  assert.deepEqual(Array.from((await after)!), [0xf0, 9, 0xf7]);
  assert.equal(await t.recv(1000, AbortSignal.abort()), null);
});

test('recv returns null after its timeout', async () => {
  const f = fake();
  const t = await WebMidiTransport.open(f.input, f.output, f.access);
  assert.equal(await t.recv(10), null);
});

test('a port that disconnects fails the pending recv, later requests, and calls onLost once', async () => {
  const f = fake();
  const lost: Error[] = [];
  const t = await WebMidiTransport.open(f.input, f.output, f.access, (e) => lost.push(e));
  const pending = t.recv(60_000);
  const gone = { ...f.output, state: 'disconnected' };
  f.access.onstatechange?.({ port: gone });
  f.access.onstatechange?.({ port: gone });
  await assert.rejects(pending, /Elektron Digitakt II.*disconnected/);
  await assert.rejects(t.recv(10), /disconnected/);
  assert.throws(() => t.send(Uint8Array.of(0xf0, 0xf7)), /disconnected/);
  assert.equal(lost.length, 1);
  assert.equal(f.sent.length, 0);
});

test('another port going away does not end the transport', async () => {
  const f = fake();
  const lost: Error[] = [];
  const t = await WebMidiTransport.open(f.input, f.output, f.access, (e) => lost.push(e));
  f.access.onstatechange?.({ port: { id: 'other', name: 'Other', state: 'disconnected' } });
  t.send(Uint8Array.of(0xf0, 0xf7));
  assert.deepEqual(f.sent, [[0xf0, 0xf7]]);
  assert.equal(lost.length, 0);
});

test('close hands the state handler back', async () => {
  const f = fake();
  const earlier = () => {};
  f.access.onstatechange = earlier;
  const t = await WebMidiTransport.open(f.input, f.output, f.access);
  t.close();
  assert.equal(f.access.onstatechange, earlier);
  assert.equal(f.input.onmidimessage, null);
});

const nav = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
afterEach(() => {
  if (nav) Object.defineProperty(globalThis, 'navigator', nav);
  else delete (globalThis as { navigator?: unknown }).navigator;
});
const withNavigator = (n: unknown) => Object.defineProperty(globalThis, 'navigator', { value: n, configurable: true });

test('a browser without Web MIDI gets a clear message', async () => {
  withNavigator({});
  await assert.rejects(requestAccess(), { message: NO_WEBMIDI });
  assert.match(NO_WEBMIDI, /Use Chrome or Edge, or the desktop app/);
});

test('a denied SysEx permission gets a clear message', async () => {
  for (const name of ['SecurityError', 'NotAllowedError']) {
    withNavigator({ requestMIDIAccess: () => Promise.reject(Object.assign(new Error('denied'), { name })) });
    await assert.rejects(requestAccess(), { message: SYSEX_DENIED });
  }
});

test('access is requested with SysEx', async () => {
  const f = fake();
  let asked: unknown;
  withNavigator({
    requestMIDIAccess: (o: unknown) => {
      asked = o;
      return Promise.resolve(f.access);
    },
  });
  assert.equal(await requestAccess(), f.access);
  assert.deepEqual(asked, { sysex: true });
});

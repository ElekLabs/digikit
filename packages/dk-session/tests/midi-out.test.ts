import assert from 'node:assert/strict';
import { test } from 'vitest';
import { emuMidiSink, MidiOut, type EmuRpcCall, type MidiSink } from '../src/index.ts';

function recorder(delayMs = 0) {
  const sent: number[][] = [];
  const sink: MidiSink = {
    async send(bytes) {
      // a slow port: a message that started is the only thing that moves until it ends
      await new Promise((r) => setTimeout(r, delayMs));
      sent.push(Array.from(bytes));
    },
  };
  return { sent, sink };
}

test('channel messages carry the right status, data bytes and channel', async () => {
  const { sent, sink } = recorder();
  const out = new MidiOut(sink);
  await out.note(1, 60, 100);
  await out.noteOff(16, 60);
  await out.cc(3, 74, 127);
  await out.start();
  await out.stop();
  assert.deepEqual(sent, [[0x90, 60, 100], [0x8f, 60, 0], [0xb2, 74, 127], [0xfa], [0xfc]]);
});

test('an NRPN is CC 99, 98, 6 and 38 with the 14-bit value split in 7-bit halves', async () => {
  const { sent, sink } = recorder();
  await new MidiOut(sink).nrpn(2, 1, 20, 0x2abc);
  assert.deepEqual(sent, [
    [0xb1, 99, 1],
    [0xb1, 98, 20],
    [0xb1, 6, 0x55],
    [0xb1, 38, 0x3c],
  ]);
});

test('out-of-range values are refused before anything is sent', async () => {
  const { sent, sink } = recorder();
  const out = new MidiOut(sink);
  await assert.rejects(out.note(0, 60, 100), /channel/);
  await assert.rejects(out.note(1, 128, 100), /note/);
  await assert.rejects(out.note(1, 60, 0), /velocity/);
  await assert.rejects(out.cc(1, 1, 128), /value/);
  await assert.rejects(out.nrpn(1, 1, 1, 0x4000), /NRPN value/);
  assert.deepEqual(sent, []);
});

test('concurrent calls stay in call order and an NRPN is never split by another message', async () => {
  const { sent, sink } = recorder(2);
  const out = new MidiOut(sink);
  await Promise.all([out.cc(1, 7, 1), out.nrpn(1, 1, 36, 5), out.note(1, 60, 90), out.cc(1, 7, 2)]);
  assert.deepEqual(
    sent.map((m) => m[1]),
    [7, 99, 98, 6, 38, 60, 7],
  );
});

test('a failed send fails its own call and the next call still goes out', async () => {
  const sent: number[][] = [];
  let fail = true;
  const out = new MidiOut({
    async send(b) {
      if (fail) {
        fail = false;
        throw new Error('port closed');
      }
      sent.push(Array.from(b));
    },
  });
  await assert.rejects(out.start(), /port closed/);
  await out.stop();
  assert.deepEqual(sent, [[0xfc]]);
});

test('the emulator sink hands hex bytes to midi.usb.in and reports a wedged controller', async () => {
  const calls: { method: string; params?: Record<string, unknown> }[] = [];
  let queued = 1;
  const call: EmuRpcCall = async <T>(method: string, params?: Record<string, unknown>) => {
    calls.push({ method, params });
    return { queued } as T;
  };
  const out = new MidiOut(emuMidiSink(call));
  await out.note(2, 36, 100);
  assert.deepEqual(calls, [{ method: 'midi.usb.in', params: { bytes: '912464' } }]);
  queued = 0;
  await assert.rejects(out.cc(1, 1, 1), /wedged/);
});

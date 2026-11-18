// parsePing: the model name in the reply of the stock MidiRpc Ping (`u8 blobLen`, the blob, NUL-ended strings).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { parsePing } from '../src/identify.ts';

const text = (...parts: string[]) => Uint8Array.from(Buffer.from(parts.map((p) => `${p}\0`).join('')));
const reply = (blob: number[], ...strings: string[]) => Uint8Array.from([blob.length, ...blob, ...text(...strings)]);

test('parsePing takes the first string after the blob and ignores what follows', () => {
  assert.equal(parsePing(reply([1, 2, 3, 4], 'Some Device', 'second')), 'Some Device');
  const trailing = Uint8Array.from([...reply([1, 2], 'Name', 'second'), 0x2a]);
  assert.equal(parsePing(trailing), 'Name');
});

test('parsePing with an empty blob reads the name at byte 1', () => {
  assert.equal(parsePing(reply([], 'Name')), 'Name');
});

test('parsePing finds the name behind the extra length byte of a real Syntakt reply', () => {
  // `1e 16 <22 request types> "Syntakt" 00`, as the emulated Syntakt 1.42 answered Ping
  const types = [
    1, 2, 3, 4, 6, 7, 9, 0x50, 0x52, 0x51, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e,
  ];
  assert.equal(types.length, 22);
  assert.equal(parsePing(Uint8Array.from([0x1e, 22, ...types, ...Buffer.from('Syntakt\0')])), 'Syntakt');
});

test('parsePing gives undefined for an empty, cut or nameless reply', () => {
  assert.equal(parsePing(new Uint8Array()), undefined);
  assert.equal(parsePing(Uint8Array.from([5, 1, 2])), undefined); // the blob is cut
  assert.equal(parsePing(Uint8Array.from([2, 1, 2])), undefined); // no name after the blob
  assert.equal(parsePing(Uint8Array.from([0, ...Buffer.from('Name')])), undefined); // no NUL
  assert.equal(parsePing(Uint8Array.from([0, 0, 0])), undefined); // empty name
});

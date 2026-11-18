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

test('parsePing gives undefined for an empty, cut or nameless reply', () => {
  assert.equal(parsePing(new Uint8Array()), undefined);
  assert.equal(parsePing(Uint8Array.from([5, 1, 2])), undefined); // the blob is cut
  assert.equal(parsePing(Uint8Array.from([2, 1, 2])), undefined); // no name after the blob
  assert.equal(parsePing(Uint8Array.from([0, ...Buffer.from('Name')])), undefined); // no NUL
  assert.equal(parsePing(Uint8Array.from([0, 0, ...Buffer.from('x\0')])), undefined); // empty name
});

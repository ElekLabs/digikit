// parsePing: the model name in the reply of the stock MidiRpc Ping, on the vectors shared with the Rust parser (tests/fixtures/ping-vectors.json):
// the plain layout (`u8 blobLen`, the blob, NUL-ended strings) and the Syntakt's length-prefixed one (`u8 rest` in front).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'vitest';
import { parsePing } from '../src/identify.ts';

interface Case {
  name: string;
  payload: string;
  model: string | null;
}
const fixture = path.join(import.meta.dirname, '../../../tests/fixtures/ping-vectors.json');
const cases = (JSON.parse(fs.readFileSync(fixture, 'utf8')) as { cases: Case[] }).cases;

test('the shared Ping vectors', () => {
  assert.ok(cases.length >= 8);
  for (const c of cases) {
    assert.equal(parsePing(Uint8Array.from(Buffer.from(c.payload, 'hex'))) ?? null, c.model, c.name);
  }
});

// parsePing: the model name in the reply of the stock MidiRpc Ping, on the vectors shared with the Rust parser (tests/fixtures/ping-vectors.json):
// the plain layout (`u8 blobLen`, the blob, NUL-ended strings) and the Syntakt's length-prefixed one (`u8 rest` in front).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'vitest';
import type { DkInfoFull } from '../src/dkupload-core.ts';
import { identifyDevice, parsePing, RPC_PING } from '../src/identify.ts';

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

test('a runtime panel identifies a versioned INFO port without a stock RPC reply', async () => {
  const session = {
    info: async () => ({ port: 'example-1.12', runtime: 256 }) as DkInfoFull,
    rpc: async (): Promise<Uint8Array> => {
      throw new Error('stock RPC unavailable');
    },
  };
  const result = await identifyDevice(
    session,
    { devices: [{ id: 'example', name: 'Example' }] },
    { preferRuntimeInfo: true },
  );
  assert.equal(result.id, 'example');
  assert.equal(result.info?.port, 'example-1.12');
});

test('a runtime panel still identifies stock firmware when INFO times out', async () => {
  let probes = 0;
  const session = {
    info: async (): Promise<DkInfoFull> => {
      probes++;
      throw Object.assign(new Error('timeout'), { kind: 'timeout' });
    },
    rpc: async (type: number) => (type === RPC_PING ? Uint8Array.of(0, 65, 0) : new TextEncoder().encode('1.12')),
  };
  const result = await identifyDevice(
    session,
    { devices: [{ id: 'example', name: 'A' }] },
    { preferRuntimeInfo: true },
  );
  assert.equal(result.id, 'example');
  assert.equal(result.os, '1.12');
  assert.equal(result.info, null);
  assert.equal(probes, 1);
});

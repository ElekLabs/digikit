// The MODULES reply: the decoder reads the bytes the runtime's encoder test wrote (tests/fixtures/protocol/modules-reply.json): the fixed part ends at dsp_len, then the
// sections of the services that hold something for the record.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { decodeModulesPage, serviceHash } from '../src/index.ts';

const fixture = JSON.parse(
  readFileSync(new URL('../../../tests/fixtures/protocol/modules-reply.json', import.meta.url), 'utf8'),
) as { hex: string; total: number; modules: unknown[] };

test('the decoder reads the records and the sections of the fixture reply', () => {
  const body = Uint8Array.from(Buffer.from(fixture.hex, 'hex'));
  const page = decodeModulesPage(body);
  assert.equal(page.total, fixture.total);
  // a record with no Workbench token has no `uiInstance` (the fixture says null)
  const want = (fixture.modules as object[]).map((m) =>
    Object.fromEntries(Object.entries(m).filter(([, v]) => v !== null)),
  );
  assert.deepEqual(JSON.parse(JSON.stringify(page.modules)), want);
});

test('a service name hashes as the runtime does (FNV-1a 32)', () => {
  assert.equal(serviceHash(''), 0x811c9dc5);
  assert.equal(serviceHash('foobar'), 0xbf9cf968);
});

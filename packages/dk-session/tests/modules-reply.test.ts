// The MODULES reply: the decoder reads the bytes the runtime's encoder test wrote (tests/fixtures/protocol/modules-reply.json): the fixed part ends at dsp_len, then the
// sections of the services that hold something for the record.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import { decodeModulesPage, serviceHash } from '../src/index.ts';
import { OLD_MODULES } from '../src/modules-reply.ts';
import { CAP } from '../src/gen_proto.ts';

const NEW_CAPS = (1 << CAP.MODULES) | (1 << CAP.MODULE_SECTIONS);

const fixture = JSON.parse(
  readFileSync(new URL('../../../tests/fixtures/protocol/modules-reply.json', import.meta.url), 'utf8'),
) as { hex: string; total: number; modules: unknown[] };

test('the decoder reads the records and the sections of the fixture reply', () => {
  const body = Uint8Array.from(Buffer.from(fixture.hex, 'hex'));
  const page = decodeModulesPage(body, NEW_CAPS);
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

test('a reply of a device without the MODULE_SECTIONS capability is refused as an older layout, never read as the new one', () => {
  // an older record (runtime 0.11.4): the fixed part up to dsp_len is the same bytes as far as it goes, then u32 rows, u8 n_types, the types, u32 faults, u8 flags
  const old = new Uint8Array(18 + 71 + 2);
  old[16] = 1;
  old[17] = 1;
  old[18 + 61 + 4] = 2; // n_types
  assert.throws(() => decodeModulesPage(old, 1 << CAP.MODULES), new RegExp(OLD_MODULES));
  assert.throws(() => decodeModulesPage(old, null), new RegExp(OLD_MODULES));
  assert.throws(() => decodeModulesPage(Uint8Array.from(Buffer.from(fixture.hex, 'hex')), 1 << CAP.MODULES), /Update the firmware/);
});

import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { groupOf, modulePath } from '../src/modules/module-path.ts';

test('a file without a header table is a Tool', () => {
  assert.equal(groupOf(new Uint8Array(16)), 'Tools');
});

test('modulePath puts the module in its kind folder, the id with "-"', () => {
  assert.equal(modulePath('acme/sine', 'Machines'), 'modules/machine/acme-sine.dkm');
  assert.equal(modulePath('acme/sine', 'Services'), 'modules/tool/acme-sine.dkm');
  assert.equal(modulePath('acme/sine'), 'modules/tool/acme-sine.dkm');
  assert.equal(modulePath('acme/sine', 'Effects', 'mine'), 'modules/fx/mine.dkm');
});

test('modulePath refuses an id or a path that is too long', () => {
  assert.throws(() => modulePath('a/' + 'x'.repeat(40)), DkError);
  assert.throws(() => modulePath('a/b', 'LFO shapes', 'x'.repeat(40)), DkError);
});

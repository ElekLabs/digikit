import assert from 'node:assert/strict';
import { test } from 'vitest';
import { pgoWarning, type BuildInfo } from '../src/pgo-warning.ts';

test('the loaded firmware selects the DSP core warning independently of host PGO', () => {
  const info: BuildInfo = { pgo: 'ok', dsp_pgo: { 'unit-a': 'plain', 'unit-b': 'ok' } };

  assert.equal(pgoWarning(info, undefined), undefined);
  const warning = pgoWarning(info, 'unit-a');
  assert.equal(warning?.label, 'no DSP PGO');
  assert.deepEqual(warning?.title.split('\n'), [
    'The DSP core for this firmware was built without a valid PGO profile.',
    'Run mise run perf:pgo:unit-a and relaunch.',
  ]);
  assert.equal(pgoWarning(info, 'unit-b'), undefined);
  assert.equal(pgoWarning(info, 'unit-a', false), undefined);
});

test('a plain host appears as detail only when the selected core also needs a profile', () => {
  const info: BuildInfo = { pgo: 'missing', dsp_pgo: { 'unit-a': 'ok', 'unit-b': 'plain' } };

  assert.equal(pgoWarning(info, 'unit-a'), undefined);
  const warning = pgoWarning(info, 'unit-b');
  assert.equal(warning?.label, 'no DSP PGO');
  assert.match(warning?.title ?? '', /The emulator host was built without a valid PGO profile\./);
  assert.match(warning?.title ?? '', /mise run perf:pgo:unit-b/);
});

test('external, unavailable and unreported cores stay quiet', () => {
  const info: BuildInfo = { pgo: 'stale', dsp_pgo: { 'unit-a': 'external', 'unit-b': 'unavailable' } };

  assert.equal(pgoWarning(info, 'unit-a'), undefined);
  assert.equal(pgoWarning(info, 'unit-b'), undefined);
  assert.equal(pgoWarning(info, 'other'), undefined);
});

test('explicitly opting out stays quiet', () => {
  const dsp_pgo = { 'unit-a': 'plain' } as const;

  assert.equal(pgoWarning({ pgo: 'off', dsp_pgo }, 'unit-a'), undefined);
  assert.equal(pgoWarning({ pgo: 'skipped', dsp_pgo }, 'unit-a'), undefined);
});

test('old build info warns about a known host failure only once firmware is loaded', () => {
  const info = { pgo: 'mismatch', pgo_detail: 'changed: sources' };

  assert.equal(pgoWarning(info, undefined), undefined);
  const warning = pgoWarning(info, 'unit-a');
  assert.equal(warning?.label, 'no host PGO');
  assert.deepEqual(warning?.title.split('\n'), [
    'The host PGO profile does not match this build.',
    'changed: sources.',
    'Run mise run perf:pgo:both and relaunch.',
  ]);
});

test('old build info stays quiet when the host state is healthy or unknown', () => {
  assert.equal(pgoWarning(undefined, 'unit-a'), undefined);
  assert.equal(pgoWarning({ pgo: null }, 'unit-a'), undefined);
  assert.equal(pgoWarning({}, 'unit-a'), undefined);
  assert.equal(pgoWarning({ pgo: 'ok' }, 'unit-a'), undefined);
  assert.equal(pgoWarning({ pgo: 'something-new' }, 'unit-a'), undefined);
});

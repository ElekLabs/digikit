import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createRoot } from 'solid-js';
import { memoryKeyValue } from '../src/storage';
import {
  calibrationChip,
  checkCalibration,
  createActiveCalibrations,
  estimateDsp,
} from '../src/profile/calibration-active';
import type { CalibrationDoc } from '../src/profile/calibration-view';
import type { DspView } from '../src/profile/profile-core';

const doc = (over: Partial<CalibrationDoc> = {}): CalibrationDoc => ({
  schema: 'dk.calibration/1',
  key: { device: 'dev-a', device_unit: 'cycles', block_cycles: 100_000 },
  dsp: { factor: { fit: 1.3, budget: 1.4 }, error: { n: 3, mean: 0.01, worst_under: 0.013 } },
  ...over,
});
const text = (d: unknown) => JSON.stringify(d);

test('a calibration is checked for schema, a DSP fit, the device and a device unit', () => {
  assert.ok(checkCalibration(text(doc()), 'dev-a').doc);
  assert.match(checkCalibration('{', 'dev-a').error!, /JSON/);
  assert.match(checkCalibration(text({ schema: 'dk.profile/1' }), 'dev-a').error!, /dk.calibration\/1/);
  assert.match(checkCalibration(text(doc({ dsp: undefined })), 'dev-a').error!, /no DSP fit/);
  assert.match(checkCalibration(text(doc()), 'dev-b').error!, /calibration of dev-a, the target is dev-b/);
  assert.match(checkCalibration(text(doc()), undefined).error!, /not known/);
  assert.match(
    checkCalibration(text(doc({ key: { device: 'dev-a', device_unit: 'insn' } })), 'dev-a').error!,
    /itself/,
  );
});

test('the store keeps one calibration per device, tells when it changes and forgets on clear', () =>
  createRoot((dispose) => {
    const kv = memoryKeyValue();
    const a = createActiveCalibrations(kv);
    assert.equal(a.get('dev-a'), undefined);
    assert.match(a.load('x.json', text(doc()), 'dev-b')!, /x.json: the file is a calibration of dev-a/);
    assert.equal(a.load('x.json', text(doc()), 'dev-a'), undefined);
    assert.equal(a.get('dev-a')?.name, 'x.json');
    assert.equal(a.get('dev-b'), undefined);
    // a new store over the same KeyValue (the next start) finds it
    assert.equal(createActiveCalibrations(kv).get('dev-a')?.doc.dsp?.factor.budget, 1.4);
    a.clear('dev-a');
    assert.equal(a.get('dev-a'), undefined);
    assert.equal(createActiveCalibrations(kv).get('dev-a'), undefined);
    dispose();
  }));

test('the estimate is instructions times the budget factor, against the measured block', () => {
  const view: DspView = {
    blocks: 10,
    total: 3000,
    modules: [
      { name: 'a', kind: 'machine', insnPerBlock: 1000, top: [] },
      { name: 'b', kind: 'effect', insnPerBlock: 2000, top: [] },
    ],
  };
  const e = estimateDsp(view, doc())!;
  assert.deepEqual(
    e.rows.map((r) => [r.name, Math.round(r.cycles), r.share]),
    [
      ['a', 1400, 0.014],
      ['b', 2800, 0.028],
    ],
  );
  assert.equal(Math.round(e.total.cycles), 4200);
  assert.equal(Math.round(e.fitCycles), 3900);
  const none = estimateDsp(view, doc({ key: { device: 'dev-a' } }))!;
  assert.equal(none.rows[0]!.share, undefined);
  assert.equal(calibrationChip('Dev A 1.0', doc()), 'calibrated: Dev A 1.0, fit 1.30 ±1.3 %');
});

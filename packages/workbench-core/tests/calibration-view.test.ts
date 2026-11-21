import assert from 'node:assert/strict';
import { test } from 'vitest';
import {
  captureSetFile,
  expandCaptures,
  fitRequest,
  groupByCase,
  fitView,
  loadCalibrationFiles,
  parseRunLine,
  phaseLine,
  progressLine,
  readCaptureSet,
  secondsLeft,
  sideOf,
  tableRows,
  type CalibrationDoc,
  type CalProfileDoc,
} from '../src/profile/calibration-view';
import { runCalibration } from '../src/calibration/runner';

import { MidiOut, type Clock } from '@digi/dk-session';

test('a fit input file expands to its captures: a calcaptures file to its list, a profile to itself', () => {
  const a = { schema: 'dk.profile/1', run: { case_id: 'a' } };
  const b = { schema: 'dk.profile/1', run: { case_id: 'b' } };
  assert.deepEqual(expandCaptures({ schema: 'dk.calcaptures/1', captures: [a, b] }), [a, b]);
  assert.deepEqual(expandCaptures(a), [a]);
  assert.throws(() => expandCaptures({ schema: 'dk.calcaptures/1' }), /without captures/);
});

test('a calcaptures file of the wrong side or with a malformed capture is refused naming the file', () => {
  const a = { schema: 'dk.profile/1', run: { case_id: 'a' } };
  assert.throws(
    () => expandCaptures({ schema: 'dk.calcaptures/1', side: 'emu', captures: [a] }, 'dev', 'x.json'),
    /x\.json holds the emulator side; pass it with --model/,
  );
  assert.deepEqual(expandCaptures({ schema: 'dk.calcaptures/1', side: 'dev', captures: [a] }, 'dev', 'x.json'), [a]);
  assert.throws(
    () => expandCaptures({ schema: 'dk.calcaptures/1', captures: [null] }, 'dev', 'y.json'),
    /y\.json: capture 0/,
  );
});

const cap = (unit: 'insn' | 'cycles', caseId: string, rows: [number, number, number][]): CalProfileDoc => ({
  schema: 'dk.profile/1',
  run: { id: `${unit === 'insn' ? 'emu' : 'dev'}-1-${caseId}`, case_id: caseId, target: 'dn2' },
  dsp: { unit, blocks: 100, max_stock_block_delta: 5 },
  dsp_rows: rows.map(([row, max_call, max_block]) => ({ row, kind: 'machine', calls: 10, max_call, max_block })),
});

const fit: CalibrationDoc = {
  schema: 'dk.calibration/1',
  key: { device: 'dn2' },
  dsp: {
    factor: { fit: 1.3167, budget: 1.34 },
    error: { n: 9, mean: 0, worst_under: 0.018, loo_worst: null, loo_budget_under: null },
    steady: { n: 0, mean: 0, worst_under: 0 },
    pairs: [{ case: 'bd', id: 'row 3', stat: 'max_block', model: 1000, device: 1300, ratio: 1.3, error: 0.0127 }],
  },
  refused: [{ section: 'case', case: 'lp4', id: 'device', reason: 'captures differ in input hash' }],
  warnings: ['block cycles unknown'],
};

test('a run on one side fills its column; the other side and the fit fill the rest of the same row', () => {
  const emu = [cap('insn', 'bd', [[3, 900, 1000]]), cap('insn', 'bd', [[3, 950, 990]])];
  const dev = [cap('cycles', 'bd', [[3, 1200, 1300]])];
  const alone = tableRows({ emu });
  assert.equal(alone.length, 2);
  const block = alone.find((r) => r.label === 'bd · row 3 · block')!;
  assert.equal(block.emu, 1000); // the largest of the captures
  assert.equal(block.dev, undefined);
  assert.equal(block.factor, undefined);
  const both = tableRows({ emu, dev }, fit).find((r) => r.label === 'bd · row 3 · block')!;
  assert.deepEqual([both.emu, both.dev, both.factor, both.error], [1000, 1300, 1.3, 0.0127]);
  // a row of the call statistic has no pair in this fit: no factor
  assert.equal(tableRows({ emu, dev }, fit).find((r) => r.label.endsWith('call'))!.factor, undefined);
  assert.deepEqual(
    groupByCase(tableRows({ emu, dev }, fit)).map((g) => [g.case, g.rows.map((r) => r.what)]),
    [['bd', ['row 3 · block', 'row 3 · call']]],
  );
});

test('rows without calls are left out; the side comes from the DSP unit', () => {
  const c = cap('cycles', 'x', [[1, 0, 0]]);
  c.dsp_rows![0]!.calls = 0;
  assert.deepEqual(tableRows({ dev: [c] }), []);
  assert.equal(sideOf(c), 'dev');
  assert.equal(sideOf({ run: { id: 'emu-2026' } }), 'emu');
  assert.equal(sideOf({}), undefined);
});

test('the fit view: headline, null leave-one-out as n/a, refusals and warnings', () => {
  const v = fitView(fit);
  assert.equal(v.headline, 'fit 1.32 · budget 1.34 · worst under 1.8 % · n 9');
  assert.deepEqual(v.details, ['leave-one-out: n/a (one row only)']);
  assert.deepEqual(v.refused, ['case lp4 device: captures differ in input hash']);
  assert.deepEqual(v.warnings, ['block cycles unknown']);
  assert.equal(v.hasFit, true);
  const loo = fitView({
    ...fit,
    dsp: { ...fit.dsp!, error: { ...fit.dsp!.error, loo_worst: 0.021, loo_budget_under: 0 } },
  });
  assert.equal(loo.details[0], 'leave-one-out: worst under 2.1 %, budget under 0.0 %');
  const none = fitView({ refused: [], warnings: [] });
  assert.equal(none.hasFit, false);
  assert.match(none.headline, /no DSP pairs/);
});

test('a capture set file round-trips; loose dk.profile files give one set; mixed sides and foreign files are refused by name', () => {
  const emu = [cap('insn', 'bd', [[3, 1, 2]]), cap('insn', 'stock', [])];
  const f = captureSetFile({
    profile: 'heavy',
    captures: emu.map((profile) => ({ profile: profile as unknown as Record<string, unknown> })),
  });
  assert.equal(f.name, 'calibration-heavy-emu.captures.json');
  const back = readCaptureSet([{ name: f.name, text: f.text }]);
  assert.equal(back.side, 'emu');
  assert.equal(back.profile, 'heavy');
  assert.equal(back.captures.length, 2);
  const loose = readCaptureSet(emu.map((c, i) => ({ name: `c${i}.profile.json`, text: JSON.stringify(c) })));
  assert.equal(loose.source, '2 files');
  assert.throws(
    () =>
      readCaptureSet([
        { name: 'a.json', text: JSON.stringify(emu[0]) },
        { name: 'b.json', text: JSON.stringify(cap('cycles', 'bd', [])) },
      ]),
    /a\.json, b\.json: the captures are not all/,
  );
  assert.throws(() => readCaptureSet([{ name: 'x.json', text: '{"schema":"other"}' }]), /x\.json: neither/);
  assert.throws(() => readCaptureSet([{ name: 'y.json', text: 'nope' }]), /y\.json: not JSON/);
  assert.throws(
    () => readCaptureSet([{ name: 'z.json', text: JSON.stringify({ schema: 'dk.profile/1', run: {} }) }]),
    /no run.case_id/,
  );
});

test('a fit pairs an emulator set with a device set and refuses two of one side', () => {
  const e = readCaptureSet([{ name: 'e.json', text: JSON.stringify(cap('insn', 'bd', [])) }]);
  const d = readCaptureSet([{ name: 'd.json', text: JSON.stringify(cap('cycles', 'bd', [])) }]);
  const req = fitRequest(e, d);
  assert.equal(req.model.length, 1);
  assert.deepEqual(req.sources, { model: 'e.json', device: 'd.json' });
  assert.throws(() => fitRequest(e, e), /one emulator run with one device run/);
});

test('progress: the runner log lines parse, and the time left counts the later captures and cases', () => {
  assert.deepEqual(parseRunLine('case lp4: capture 2 of 3'), { kind: 'capture', case: 'lp4', n: 2, of: 3 });
  assert.deepEqual(parseRunLine('case lp4: skipped, it presses keys'), {
    kind: 'skip',
    case: 'lp4',
    reason: 'it presses keys',
  });
  assert.deepEqual(parseRunLine('loading a/b'), { kind: 'text', text: 'loading a/b' });
  const { profiles } = loadCalibrationFiles({
    profiles: {
      'p/light.toml': `schema = "dk.calprofile/1"\nname = "light"\nwarmup_s = 1\ncapture_s = 4\nrepeats = 3\n[[case]]\nid = "stock"\nsteps = "s"\n[[case]]\nid = "lp4"\nsteps = "s"\n`,
    },
    steps: {},
    cases: {},
  });
  const p = profiles[0]!;
  // capture 2 of 3 of the first case, 2 s in: 3 s here, one more capture (5 s), one whole later case (15 s)
  assert.equal(secondsLeft({ profile: p, case: 'stock', n: 2, intoCaptureS: 2 }), 3 + 5 + 15);
  assert.equal(secondsLeft({ profile: p, case: 'lp4', n: 3, intoCaptureS: 9 }), 0);
  assert.equal(progressLine({ case: 'lp4', n: 2, of: 3, left: 6 }), 'case lp4 · capture 2/3 · 6 s left');
});

test('phase lines say what the run does between captures', () => {
  assert.deepEqual(parseRunLine('case lp4: restarting'), { kind: 'restart', case: 'lp4' });
  assert.deepEqual(parseRunLine('case lp4: putting the stock machine back'), { kind: 'restore', case: 'lp4' });
  assert.equal(phaseLine({ kind: 'restart', case: 'lp4' }), 'restarting for case lp4');
  assert.equal(phaseLine({ kind: 'stable', waited: 4, of: 30 }), 'waiting for a stable boot 4 s / 30 s');
  assert.equal(phaseLine({ kind: 'restore', case: 'lp4' }), 'putting the stock machine back');
});

test('the progress parser follows what the shared runner logs', async () => {
  const { profiles, steps } = loadCalibrationFiles({
    profiles: {
      'light.toml': `schema = "dk.calprofile/1"\nname = "light"\nwarmup_s = 0.1\ncapture_s = 0.1\nrepeats = 2\n[[case]]\nid = "stock"\nsteps = "s"\n[[case]]\nid = "k"\nsteps = "keys"\n`,
    },
    steps: {
      'a.toml': `schema = "dk.steps/1"\nname = "s"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n`,
      'b.toml': `schema = "dk.steps/1"\nname = "keys"\n[[step]]\naction = "key"\nkey = "play"\n`,
    },
    cases: {},
  });
  const lines: string[] = [];
  const clock: Clock = { now: async () => 0, pause: async () => {} };
  await runCalibration({
    profile: profiles[0]!,
    steps,
    rig: {
      midi: new MidiOut({ send: async () => {} }),
      clock,
      stats: {
        capabilities: async () => ({ dsp: false, spans: true }),
        start: async () => {},
        clear: async () => ({}),
        stop: async () => {},
        read: async () => ({}),
      },
    },
    midi: { trackChannels: [1], params: {} },
    run: { id: 'r', target: 't', firmware_sha256: 'ff' },
    dspUnit: 'insn',
    log: (l) => lines.push(l),
  });
  const events = lines.map(parseRunLine);
  assert.deepEqual(
    events.filter((e) => e.kind === 'capture'),
    [
      { kind: 'capture', case: 'stock', n: 1, of: 2 },
      { kind: 'capture', case: 'stock', n: 2, of: 2 },
    ],
  );
  assert.equal(events.filter((e) => e.kind === 'skip').length, 1);
});

test('malformed captures are refused with a message naming the file', () => {
  const ok = cap('insn', 'bd', [[3, 1, 2]]);
  const bad = (name: string, edit: (d: Record<string, unknown>) => void, re: RegExp) => {
    const d = JSON.parse(JSON.stringify(ok)) as Record<string, unknown>;
    edit(d);
    assert.throws(() => readCaptureSet([{ name, text: JSON.stringify(d) }]), re);
  };
  bad('a.json', (d) => (d.dsp_rows = 'x'), /a\.json: capture bd: dsp_rows is not a list/);
  bad(
    'b.json',
    (d) => (d.dsp_rows = [{ row: 1, calls: 'many', max_call: 1, max_block: 1 }]),
    /b\.json: capture bd: a dsp row lacks numbers/,
  );
  bad('c.json', (d) => (d.dsp_rows = [null]), /c\.json: capture bd: a dsp row lacks numbers/);
  bad('d.json', (d) => (d.dsp = { unit: 5 }), /d\.json: capture bd: the dsp header is malformed/);
  bad('e.json', (d) => (d.spans = [{ owner: 1 }]), /e\.json: capture bd: a span lacks numbers/);
  bad('f.json', (d) => (d.spans = {}), /f\.json: capture bd: spans is not a list/);
  assert.throws(
    () => readCaptureSet([{ name: 'g.json', text: JSON.stringify({ schema: 'dk.calcaptures/1', captures: [7] }) }]),
    /g\.json: a capture is not a dk.profile\/1 document/,
  );
});

test('a fit refuses two profiles; the saved run name has no path characters; skipped cases take no time', () => {
  const mk = (unit: 'insn' | 'cycles', profile: string) =>
    readCaptureSet([
      {
        name: 'x.json',
        text: JSON.stringify({ schema: 'dk.calcaptures/1', profile, captures: [cap(unit, 'bd', [])] }),
      },
    ]);
  assert.throws(
    () => fitRequest(mk('insn', 'light'), mk('cycles', 'heavy')),
    /profile light and the device run of heavy/,
  );
  assert.equal(captureSetFile({ profile: '../a b', captures: [] }).name, 'calibration-.._a_b-run.captures.json');
  const { profiles } = loadCalibrationFiles({
    profiles: {
      'p/x.toml': `schema = "dk.calprofile/1"\nname = "x"\nwarmup_s = 1\ncapture_s = 4\nrepeats = 1\n[[case]]\nid = "stock"\nsteps = "s"\n[[case]]\nid = "b"\nsteps = "s"\n[[case]]\nid = "c"\nsteps = "s"\n`,
    },
    steps: {},
    cases: {},
  });
  assert.equal(secondsLeft({ profile: profiles[0]!, case: 'stock', n: 1, intoCaptureS: 0, skipping: ['b'] }), 10);
});

test('the note before Run names the tracks and says they are left on the stock machine', async () => {
  const { trackNote } = await import('../src/profile/calibration-view');
  assert.equal(trackNote([]), '');
  assert.equal(trackNote([1]), 'Calibration uses track 1 and leaves it on the stock machine.');
  assert.equal(trackNote([1, 2]), 'Calibration uses tracks 1, 2 and leaves them on the stock machine.');
  assert.equal(trackNote([1, 2, 3, 4, 7]), 'Calibration uses tracks 1 to 4, 7 and leaves them on the stock machine.');
});

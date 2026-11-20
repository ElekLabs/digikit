import assert from 'node:assert/strict';
import { test } from 'vitest';
import { MidiOut, ModuleLoadRefused, type Clock } from '@digi/dk-session';
import type { MidiMap } from '@digi/devices';
import { addModuleCases, parseCalCase, parseCalProfile } from '../src/calibration/calprofile';
import { profileTracks, runCalibration, splitCapture, type Rig, type RunInput } from '../src/calibration/runner';
import { parseStepFile, type StepLibrary } from '../src/calibration/steps';
import type { DspStats, StatsSource } from '../src/calibration/stats';

const midi: MidiMap = {
  trackChannels: [1, 2],
  params: { amp_release: { cc: 83, nrpn: [1, 34] } },
};
const lib: StepLibrary = {
  hold: parseStepFile(
    `schema = "dk.steps/1"\nname = "hold"\n[args]\ntracks = "1"\n[[step]]\naction = "midi.note"\ntrack = "{tracks}"\nkey = 60\n`,
    'hold',
  ),
  off: parseStepFile(
    `schema = "dk.steps/1"\nname = "off"\n[[step]]\naction = "midi.note_off"\ntrack = "all"\nkey = 60\n`,
    'off',
  ),
  mach: parseStepFile(
    `schema = "dk.steps/1"\nname = "mach"\n[[step]]\naction = "setup"\ntrack = 1\nmachine = "a/b"\n`,
    'mach',
  ),
  reset: parseStepFile(
    `schema = "dk.steps/1"\nname = "reset"\n[[step]]\naction = "setup"\ntrack = 1\nmachine = 0\n`,
    'reset',
  ),
  fx: parseStepFile(`schema = "dk.steps/1"\nname = "fx"\n[[step]]\naction = "setup"\neffect = "a/fx"\n`, 'fx'),
  ring: parseStepFile(
    `schema = "dk.steps/1"\nname = "ring"\n[[step]]\naction = "setup"\ntrack = 1\nmachine = "a/b"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\nretrigger_ms = 400\n`,
    'ring',
  ),
  win: parseStepFile(
    `schema = "dk.steps/1"\nname = "win"\n[[step]]\naction = "capture"\nmark = "start"\n[[step]]\naction = "midi.cc"\ntrack = 1\nparam = "amp_release"\nvalue = 9\n[[step]]\naction = "wait"\nms = 700\n[[step]]\naction = "capture"\nmark = "stop"\n`,
    'win',
  ),
};

const sweep = (calls: number): DspStats => ({
  on: true,
  generation: 1,
  counts: { sweeps: 1, gaps: 0, ignored: 0 },
  sweep: {
    number: 1,
    blocks: 100,
    maxStockBlockDelta: 7,
    overruns: 0,
    rows: [
      { row: 0, kind: 'machine', calls, sumCycles: BigInt(calls) * 10n, maxCall: 12, maxBlock: 13 },
      { row: 9, kind: 'effect', calls: 0, sumCycles: 0n, maxCall: 0, maxBlock: 0 },
    ],
  },
});

/** One recorder of everything the runner does, in order. */
function fake(opts: { moduleCalls?: number; loaded?: string[]; sweepGeneration?: number } = {}) {
  const log: string[] = [];
  let loaded = opts.loaded ?? [];
  const clock: Clock = { now: async () => 0, pause: async (ms) => void log.push(`pause ${ms}`) };
  const stats: StatsSource = {
    capabilities: async () => ({ dsp: true, spans: true }),
    start: async () => void log.push('stats.start'),
    clear: async () => {
      log.push('stats.clear');
      return { generation: 1 };
    },
    stop: async () => void log.push('stats.stop'),
    read: async () => {
      log.push('stats.read');
      const spans = {
        spans: [{ owner: 0, kind: 'module' as const, name: 'm', calls: 1, sum_ticks: 2, max_ticks: 3 }],
        span_clock_hz: 132_000_000,
        span_elapsed_ticks: 5,
      };
      const calls = log.filter((l) => l === 'load').length > 0 && loaded.length > 0 ? (opts.moduleCalls ?? 0) : 0;
      return { spans, dsp: sweep(calls) };
    },
  };
  const sent: number[][] = [];
  const out = new MidiOut({
    async send(b) {
      sent.push(Array.from(b));
      log.push(`midi ${Array.from(b).join(',')}`);
    },
  });
  const rig: Rig = {
    midi: out,
    stats,
    clock,
    // the runtime log: DSP_SWEEP events of the generation the sweep belongs to (`sweepGeneration`, default the one `clear` answered)
    events: {
      listen: async () => void log.push('events.listen'),
      until: async (kind, match) => {
        const e = { seq: 1, kind, args: [], fields: { sweep: 3, generation: opts.sweepGeneration ?? 1, blocks: 50 } };
        log.push(`until ${kind}`);
        if (match && !match(e as never)) throw new Error('no sweep of that generation');
        return e as never;
      },
    },
    modules: {
      install: async (ids) => Object.fromEntries(ids.map((id) => [id, `sha-${id}`])),
      select: async (ids) => {
        log.push('load');
        loaded = ids;
        log.push(`select ${ids.join('+') || 'none'}`);
      },
      loaded: async () => loaded,
    },
  };
  return { rig, log, sent };
}

const base = `schema = "dk.calprofile/1"\nname = "p"\nwarmup_s = 1\ncapture_s = 2\nrepeats = 2\n`;
const run = (rig: Rig, text: string, extra: Partial<RunInput> = {}) =>
  runCalibration({
    profile: parseCalProfile(text, 'p'),
    steps: lib,
    rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
    ...extra,
  });

test('a case plays its steps once and captures `repeats` times: start, warm up, clear, capture, read, stop', async () => {
  const f = fake();
  const res = await run(f.rig, base + `teardown = { steps = "off" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`);
  assert.deepEqual(f.log, [
    'stats.stop', // counters an earlier run left on
    'midi 144,60,100',
    ...[1, 2].flatMap(() => [
      'stats.start',
      'pause 1000',
      'stats.clear',
      'pause 2000',
      'events.listen',
      'until DSP_SWEEP',
      'stats.stop',
      'stats.read',
    ]),
    'midi 128,60,0',
    'midi 129,60,0',
  ]);
  assert.equal(res.length, 1);
  assert.deepEqual(
    res[0]!.captures.map((c) => c.file),
    ['stock-1.profile.json', 'stock-2.profile.json'],
  );
});

test('a capture is a dk.profile/1 with the case, the input hash, spans and the DSP header and rows', async () => {
  const f = fake();
  const [c] = await run(f.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`);
  const p = c!.captures[0]!.profile as Record<string, any>;
  assert.equal(p.schema, 'dk.profile/1');
  assert.equal(p.run.case_id, 'stock');
  assert.equal(p.run.target, 'tgt');
  assert.equal(p.run.firmware_sha256, 'ff');
  assert.match(p.run.input_sha256, /^[0-9a-f]{64}$/);
  assert.deepEqual(p.run.module_sha256, []);
  assert.equal('stock_only' in p.run, false);
  assert.deepEqual(p.dsp, { unit: 'insn', blocks: 100, max_stock_block_delta: 7, overruns: 0 });
  // rows with no calls are left out; this is how a module-free case reads
  assert.deepEqual(p.dsp_rows, [{ row: 0, kind: 'machine', calls: 0, sum_cycles: 0, max_call: 12, max_block: 13 }]);
  assert.equal(p.spans[0].name, 'm');
  assert.equal(p.span_clock_hz, 132_000_000);
  // the repeats of a case have one input hash; another case or other args another
  const f2 = fake();
  const res = await run(
    f2.rig,
    base + `[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "two"\nsteps = "hold"\nargs = { tracks = "2" }\n`,
  );
  const h = res.map((r) => (r.captures[0]!.profile.run as { input_sha256: string }).input_sha256);
  const again = (res[0]!.captures[1]!.profile.run as { input_sha256: string }).input_sha256;
  assert.equal(h[0], again);
  assert.notEqual(h[0], h[1]);
});

test('markers make the steps between them the capture window, in place of capture_s', async () => {
  const f = fake();
  await run(f.rig, base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "win"\n`);
  assert.deepEqual(f.log, [
    'stats.stop',
    'stats.start',
    'pause 1000',
    'stats.clear',
    'midi 176,83,9',
    'pause 700',
    'events.listen',
    'until DSP_SWEEP',
    'stats.stop',
    'stats.read',
  ]);
  const ops = splitCapture(
    [{ op: 'midi.start' }, { op: 'capture', mark: 'start' }, { op: 'midi.stop' }, { op: 'capture', mark: 'stop' }],
    'c',
  );
  assert.deepEqual([ops.pre.length, ops.body.length, ops.post.length], [1, 1, 0]);
  assert.throws(() => splitCapture([{ op: 'capture', mark: 'start' }], 'c'), /both/);
});

test('modules: the stock case runs with none loaded, a module case with only its module, and the hashes are recorded', async () => {
  const f = fake({ loaded: ['a/b', 'c/d'], moduleCalls: 5 });
  const text =
    base.replace('repeats = 2', 'repeats = 1') +
    `modules = ["a/b", "c/d"]\n[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "ab"\nmodules = ["a/b"]\nsteps = "hold"\n[[case]]\nid = "both"\nmodules = ["c/d", "a/b"]\nsteps = "hold"\n`;
  const res = await run(f.rig, text);
  assert.deepEqual(
    f.log.filter((l) => l.startsWith('select')),
    ['select none', 'select a/b', 'select c/d+a/b'],
  );
  assert.deepEqual((res[0]!.captures[0]!.profile.run as { module_sha256: string[] }).module_sha256, []);
  assert.deepEqual((res[1]!.captures[0]!.profile.run as { module_sha256: string[] }).module_sha256, ['sha-a/b']);
  assert.deepEqual((res[2]!.captures[0]!.profile.run as { module_sha256: string[] }).module_sha256, [
    'sha-a/b',
    'sha-c/d',
  ]);
});

test('a case already in the right module state does not restart the target', async () => {
  const f = fake({ loaded: [] });
  await run(f.rig, base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "hold"\n`);
  assert.equal(
    f.log.some((l) => l.startsWith('select')),
    false,
  );
});

test('the stock case is refused when it counted module rows', async () => {
  const f = fake({ loaded: ['a/b'], moduleCalls: 5 });
  f.rig.modules!.select = async () => void f.log.push('load'); // the target keeps its module: loaded() still says it
  await assert.rejects(run(f.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`), /expected \[\]/);
  const g = fake();
  const counting: StatsSource = { ...g.rig.stats, read: async () => ({ dsp: sweep(4) }) };
  await assert.rejects(
    run({ ...g.rig, stats: counting }, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`),
    /counted module rows/,
  );
});

test('a case the target cannot play is skipped with its reason, the others run', async () => {
  const f = fake();
  const res = await run(
    f.rig,
    base.replace('repeats = 2', 'repeats = 1') +
      `[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "m"\nsteps = "mach"\n`,
  );
  assert.equal(res[0]!.captures.length, 1);
  assert.match(res[1]!.skipped!, /SETUP command/);
  assert.equal(res[1]!.captures.length, 0);
  // with a setter the case runs
  const calls: unknown[][] = [];
  f.rig.setup = async (...a) => void calls.push(a);
  const again = await run(
    f.rig,
    base.replace('repeats = 2', 'repeats = 1') +
      `[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "m"\nsteps = "mach"\n`,
  );
  assert.equal(again[1]!.skipped, undefined);
  assert.deepEqual(calls, [
    ['machine', 1, 0],
    ['machine', 1, 'a/b'],
  ]);
});

test('a wrong name fails before anything is played', async () => {
  const f = fake();
  await assert.rejects(run(f.rig, base + `[[case]]\nid = "stock"\nsteps = "nope"\n`), /no step file "nope"/);
  assert.deepEqual(f.log, []);
});

test('the capture ends at the DSP_SWEEP of the clear generation, from the event and not by polling', async () => {
  const f = fake();
  const [c] = await run(f.rig, base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "hold"\n`);
  assert.ok(c!.captures[0]!.profile.dsp);
  assert.equal(
    f.log.filter((l) => l.startsWith('pause')).length,
    2,
    'the warm-up and the window, and no polling pause',
  );
  // a sweep of another generation does not end the capture
  const g0 = fake({ sweepGeneration: 9 });
  await assert.rejects(run(g0.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`), /no sweep of that generation/);
  assert.ok(g0.log.includes('stats.stop'), 'and the counters are stopped');
  // a target with DSP stats but no runtime log is refused before anything is played
  const h = fake();
  delete (h.rig as { events?: unknown }).events;
  await assert.rejects(
    run(h.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`),
    /needs the runtime log|rig needs/,
  );
  assert.deepEqual(h.log, []);
});

test('spans-only targets still capture', async () => {
  const g = fake();
  g.rig.stats = {
    ...g.rig.stats,
    capabilities: async () => ({ dsp: false, spans: true }),
    read: async () => ({ spans: { spans: [], span_clock_hz: 1, span_elapsed_ticks: 1 } }),
  };
  const lines: string[] = [];
  const [d] = await run(
    g.rig,
    base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "hold"\n`,
    { log: (l) => lines.push(l) },
  );
  assert.equal('dsp' in d!.captures[0]!.profile, false);
  assert.match(lines[0]!, /DSP stats are not available/);
});

test('the teardown runs after a failed case', async () => {
  const f = fake();
  f.rig.stats.read = async () => {
    throw new Error('link lost');
  };
  await assert.rejects(
    run(f.rig, base + `teardown = { steps = "off" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`),
    /link lost/,
  );
  assert.ok(f.log.includes('midi 128,60,0'));
});

test('captures are delivered as they finish, and a later failure keeps the earlier ones', async () => {
  const f = fake();
  let reads = 0;
  const inner = f.rig.stats.read;
  f.rig.stats.read = async () => {
    // the second case fails on its first read
    if (++reads > 2) throw new Error('link lost');
    return inner();
  };
  const delivered: string[] = [];
  const cases: string[] = [];
  const text = base + `[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "two"\nsteps = "hold"\n`;
  await assert.rejects(
    run(f.rig, text, { onCapture: (c) => void delivered.push(c.file), onCase: (r) => void cases.push(r.id) }),
    (e: Error & { results?: { id: string }[] }) =>
      /link lost/.test(e.message) && e.results?.map((r) => r.id).join() === 'stock',
  );
  assert.deepEqual(delivered, ['stock-1.profile.json', 'stock-2.profile.json']);
  assert.deepEqual(cases, ['stock']);
});

test('a run with no firmware identity is refused unless allowed', async () => {
  const f = fake();
  const text = base + `[[case]]\nid = "stock"\nsteps = "hold"\n`;
  await assert.rejects(run(f.rig, text, { run: { id: 'r', target: 'tgt' } }), /no firmware identity/);
  assert.deepEqual(f.log, []);
  const [c] = await run(f.rig, text, { run: { id: 'r', target: 'tgt' }, allowUnidentifiedFirmware: true });
  assert.equal(c!.captures.length, 2);
});

test('a skipped case installs and loads nothing', async () => {
  const f = fake();
  const installed: string[][] = [];
  f.rig.modules!.install = async (ids) => {
    installed.push(ids);
    return Object.fromEntries(ids.map((id) => [id, 'h']));
  };
  // "mach" needs SETUP, which this rig lacks: its module is not installed and nothing is selected for it
  const text =
    base.replace('repeats = 2', 'repeats = 1') +
    `modules = ["a/b"]\n[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "m"\nmodules = ["a/b"]\nsteps = "mach"\n`;
  const res = await run(f.rig, text);
  assert.deepEqual(installed, []);
  assert.equal(
    f.log.some((l) => l.startsWith('select')),
    false,
  );
  assert.match(res[1]!.skipped!, /SETUP/);
});

test('after a MIDI step the runner waits until the target took it', async () => {
  const f = fake();
  f.rig.drained = async () => void f.log.push('drained');
  await run(f.rig, base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "hold"\n`);
  assert.deepEqual(f.log.slice(1, 3), ['midi 144,60,100', 'drained']);
});

test('the counters are stopped when a capture fails, and a capture is read after they stop', async () => {
  const f = fake();
  f.rig.stats.clear = async () => {
    throw new Error('clear failed');
  };
  await assert.rejects(run(f.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`), /clear failed/);
  assert.ok(f.log.includes('stats.stop'));
});

test('a profile with module cases is refused when the target has no DSP stats, before anything is written', async () => {
  const f = fake();
  f.rig.stats = { ...f.rig.stats, capabilities: async () => ({ dsp: false, spans: true }) };
  const text =
    base +
    'modules = ["a/b"]\n[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "m"\nmodules = ["a/b"]\nsteps = "hold"\n';
  const delivered: string[] = [];
  await assert.rejects(run(f.rig, text, { onCapture: (c) => void delivered.push(c.file) }), /no DSP stats/);
  assert.deepEqual([f.log, delivered], [[], []]);
  // a stock-only profile may go on with spans only
  const g = fake();
  g.rig.stats = { ...g.rig.stats, capabilities: async () => ({ dsp: false, spans: true }) };
  const [c] = await run(g.rig, base.replace('repeats = 2', 'repeats = 1') + `[[case]]\nid = "stock"\nsteps = "hold"\n`);
  assert.equal(c!.captures.length, 1);
});

const withModuleCase = (steps: string) =>
  addModuleCases(
    parseCalProfile(
      base +
        `discover = "modules"\nteardown = { steps = "off" }\nmodule_teardown = { steps = "reset" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
      'p',
    ),
    [parseCalCase(`schema = "dk.calcase/1"\nid = "m"\nmodule = "a/b"\nsteps = "${steps}"\n`, 'm')],
  );

test('a module case sets its machine through the rig, and its teardown (after the profile teardown) puts the stock machine back, also after a failure', async () => {
  const f = fake({ moduleCalls: 5 });
  const calls: unknown[][] = [];
  f.rig.setup = async (...a) => void (calls.push(a), f.log.push(`setup ${a.join(' ')}`));
  await runCalibration({
    profile: withModuleCase('mach'),
    steps: lib,
    rig: f.rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
  });
  assert.deepEqual(calls, [
    ['machine', 1, 0], // the known start
    ['machine', 1, 'a/b'],
    ['machine', 1, 0],
  ]);
  const at = (l: string) => f.log.lastIndexOf(l);
  assert.ok(at('midi 128,60,0') < at('setup machine 1 0'), 'the note is released, then the stock machine comes back');
  // a capture that fails: the teardown still plays
  const g = fake({ moduleCalls: 5 });
  const read = g.rig.stats.read;
  g.rig.stats.read = async () => {
    if (g.log.includes('select a/b')) throw new Error('the link broke');
    return read();
  };
  g.rig.setup = async (...a) => void g.log.push(`setup ${a.join(' ')}`);
  await assert.rejects(
    runCalibration({
      profile: withModuleCase('mach'),
      steps: lib,
      rig: g.rig,
      midi,
      run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
      dspUnit: 'insn',
    }),
    /the link broke/,
  );
  assert.equal(g.log.at(-1), 'setup machine 1 0');
});

test('SETUP sets no effects: a case that needs one is skipped with that reason even when the rig has a setter', async () => {
  const f = fake();
  f.rig.setup = async () => {};
  const [stock, fx] = await runCalibration({
    profile: withModuleCase('fx'),
    steps: lib,
    rig: f.rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
  });
  assert.equal(stock!.skipped, undefined);
  assert.match(fx!.skipped!, /effect/);
  assert.ok(!f.log.includes('select a/b'), 'nothing was loaded for it');
});

test('a note with retrigger_ms is pressed again every that many ms of the warm-up and the window, and not otherwise', async () => {
  const f = fake({ moduleCalls: 5 });
  f.rig.setup = async () => {};
  const profile = addModuleCases(
    parseCalProfile(
      base
        .replace('repeats = 2', 'repeats = 1')
        .replace('warmup_s = 1', 'warmup_s = 1')
        .replace('capture_s = 2', 'capture_s = 1') + `discover = "modules"\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
      'p',
    ),
    [parseCalCase('schema = "dk.calcase/1"\nid = "m"\nmodule = "a/b"\nsteps = "ring"\n', 'm')],
  );
  await runCalibration({
    profile,
    steps: lib,
    rig: f.rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
  });
  const at = f.log.indexOf('select a/b');
  const after = f.log.slice(at);
  // 1 s warm-up and 1 s window in slices of 400 ms (400, 400, 200 each), the key pressed again after each slice
  assert.deepEqual(
    after.filter((l) => l.startsWith('pause')),
    ['pause 400', 'pause 400', 'pause 200', 'pause 400', 'pause 400', 'pause 200'],
  );
  assert.equal(after.filter((l) => l === 'midi 144,60,100').length, 1 + 4, 'not pressed at the end of a wait');
  assert.equal(
    f.log.slice(0, at).filter((l) => l === 'midi 144,60,100').length,
    1,
    'the stock case presses its key once',
  );
});

test('a module capture in which no row counted a call is refused, and a case whose module the image lacks is skipped with that reason', async () => {
  const f = fake({ moduleCalls: 0 });
  f.rig.setup = async () => {};
  const withCase = (steps: string, needs: string) =>
    addModuleCases(parseCalProfile(base + `discover = "modules"\n[[case]]\nid = "stock"\nsteps = "hold"\n`, 'p'), [
      parseCalCase(`schema = "dk.calcase/1"\nid = "m"\nmodule = "a/b"\n${needs}steps = "${steps}"\n`, 'm'),
    ]);
  const input = { steps: lib, midi, run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' }, dspUnit: 'insn' };
  // a case whose capture counted no call fails as a case, with its reason; the run goes on and the next case runs
  const [, failedCase] = await runCalibration({ ...input, profile: withCase('mach', ''), rig: f.rig });
  assert.match(failedCase!.failed!, /counted no calls of any machine or effect row/);
  assert.equal(failedCase!.captures.length, 0, 'the empty capture is not kept');
  // the runtime refuses the module for an import of a core module that the case declares: skipped, the run goes on
  const g = fake();
  g.rig.setup = async () => {};
  g.rig.modules!.select = async () => {
    throw new ModuleLoadRefused(0, -7, 'sample-pool');
  };
  const [, skipped] = await runCalibration({
    ...input,
    profile: withCase('mach', 'needs = ["core/sample-pool"]\n'),
    rig: g.rig,
  });
  assert.match(skipped!.skipped!, /no core\/sample-pool/);
  // E_MISSING (a service the module imports is not in the image) with a declared need: skipped as well
  const m = fake();
  m.rig.setup = async () => {};
  m.rig.modules!.select = async () => {
    throw new ModuleLoadRefused(0, -73);
  };
  const [, again] = await runCalibration({
    ...input,
    profile: withCase('mach', 'needs = ["core/sample-pool"]\n'),
    rig: m.rig,
  });
  assert.match(again!.skipped!, /lacks a service/);
  // an undeclared refusal ends the run
  const h = fake();
  h.rig.setup = async () => {};
  h.rig.modules!.select = async () => {
    throw new ModuleLoadRefused(0, -7, 'sample-pool');
  };
  await assert.rejects(runCalibration({ ...input, profile: withCase('mach', ''), rig: h.rig }), /did not load/);
});

test('when putting the machine type back is refused the run stops, before the next case can unload the module', async () => {
  const f = fake({ moduleCalls: 5 });
  let zeros = 0;
  f.rig.setup = async (_what, _track, type) => {
    if (type === 0 && ++zeros > 1) throw new Error('SETUP refused'); // the start works, the teardown after the case does not
  };
  const profile = addModuleCases(
    parseCalProfile(
      base + `discover = "modules"\nmodule_teardown = { steps = "reset" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
      'p',
    ),
    [
      parseCalCase('schema = "dk.calcase/1"\nid = "m1"\nmodule = "a/b"\nsteps = "mach"\n', 'm1'),
      parseCalCase('schema = "dk.calcase/1"\nid = "m2"\nmodule = "c/d"\nsteps = "mach"\n', 'm2'),
    ],
  );
  await assert.rejects(
    runCalibration({
      profile,
      steps: lib,
      rig: f.rig,
      midi,
      run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
      dspUnit: 'insn',
    }),
    /putting the track's machine type back failed.*the run stops before the next case/,
  );
  assert.ok(!f.log.includes('select c/d'), 'the module of the next case was not loaded');
});

test('the module teardown takes the track of the case', () => {
  const p = addModuleCases(
    parseCalProfile(
      base + `discover = "modules"\nmodule_teardown = { steps = "reset" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
      'p',
    ),
    [parseCalCase('schema = "dk.calcase/1"\nid = "m"\nmodule = "a/b"\nsteps = "mach"\nargs = { track = 3 }\n', 'm')],
  );
  assert.deepEqual(p.cases[1]!.teardown, { steps: 'reset', args: { track: 3 } });
});

/** A rig that starts dirty: SETUP refuses a type that no loaded module owns (as the firmware does), and tracks keep what was set. */
function dirty(opts: { types?: Record<number, number>; loaded?: string[]; counting?: boolean; hold?: string[] } = {}) {
  const f = fake({ moduleCalls: 5, loaded: opts.loaded ?? [] });
  const types = new Map<number, number>(Object.entries(opts.types ?? {}).map(([k, v]) => [Number(k), v]));
  const setup: string[] = [];
  const typeOf = new Map([['a/b', 17]]);
  f.rig.setup = async (_what, track, type) => {
    const t = typeof type === 'number' ? type : typeOf.get(type)!;
    const have = await f.rig.modules!.loaded();
    const valid = t === 0 || [...typeOf].some(([id, n]) => n === t && have.includes(id));
    setup.push(`${track}:${t}${valid ? '' : ' refused'}`);
    if (!valid) throw new Error(`SETUP machine track ${track - 1} type ${t}: refused, status -46`);
    types.set(track, t);
  };
  if (opts.counting) {
    let stopped = false;
    const stop = f.rig.stats.stop;
    f.rig.stats.stop = async () => {
      stopped = true;
      return stop();
    };
    const start = f.rig.stats.start;
    f.rig.stats.start = async () => {
      if (!stopped) throw new Error('the counters were still running at the first start');
      return start();
    };
  }
  return { ...f, types, setup };
}

const dirtyProfile = () =>
  addModuleCases(
    parseCalProfile(
      base + `discover = "modules"\nmodule_teardown = { steps = "reset" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
      'p',
    ),
    [parseCalCase('schema = "dk.calcase/1"\nid = "m1"\nmodule = "a/b"\nsteps = "mach"\n', 'm1')],
  );
const runDirty = (rig: Rig) =>
  runCalibration({
    profile: dirtyProfile(),
    steps: lib,
    rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
  });

test('a project machine of a run-time type that is not loaded in this boot, extra modules and running counters do not stop the run: it starts from the stock machine', async () => {
  // track 1 of the project has run-time type 16, which no loaded module owns; another module is selected; the counters were left on
  const d = dirty({ types: { 1: 16 }, loaded: ['x/extra'], counting: true });
  const results = await runDirty(d.rig);
  assert.deepEqual(
    results.map((r) => [r.id, r.failed ?? null, r.skipped ?? null]),
    [
      ['stock', null, null],
      ['m1', null, null],
    ],
  );
  assert.equal(d.setup[0], '1:0', 'the stock machine is the first SETUP, before any module is loaded');
  assert.ok(!d.setup.some((x) => x.endsWith('refused')));
  assert.ok(d.log.indexOf('select none') >= 0, 'the extra module was replaced by the stock case selection');
  assert.equal(d.types.get(1), 0, 'the track is left on the stock machine');
});

test('a case whose modules are already the selected ones does not restart, and its tracks are still reset', async () => {
  const d = dirty({ types: { 1: 16 }, loaded: ['a/b'] });
  const full = dirtyProfile();
  await runCalibration({
    profile: { ...full, cases: full.cases.filter((c) => c.id !== 'stock') },
    steps: lib,
    rig: d.rig,
    midi,
    run: { id: 'r', target: 'tgt', firmware_sha256: 'ff' },
    dspUnit: 'insn',
  });
  assert.ok(!d.log.includes('load'), 'no restart');
  assert.equal(d.setup[0], '1:0');
  assert.equal(d.types.get(1), 0);
});

test('a track that cannot be put on the stock machine refuses the run up front, with a hint, before anything is loaded', async () => {
  const d = dirty({ loaded: ['x/extra'] });
  d.rig.setup = async () => {
    throw new Error('SETUP timed out');
  };
  await assert.rejects(
    runDirty(d.rig),
    /could not put track 1 on the stock machine before the first case.*debug module/,
  );
  assert.ok(!d.log.includes('load') && !d.log.includes('stats.start'), 'nothing was loaded or counted');
});

test('the tracks a profile uses come from its setup and its MIDI channels', () => {
  const p = dirtyProfile();
  assert.deepEqual(profileTracks(p, lib, midi), [1]);
  assert.deepEqual(
    profileTracks(
      parseCalProfile(base + `teardown = { steps = "off" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`, 'p'),
      lib,
      midi,
    ),
    [1],
  );
});

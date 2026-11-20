import assert from 'node:assert/strict';
import { test } from 'vitest';
import { MidiOut, type Clock } from '@digi/dk-session';
import type { MidiMap } from '@digi/devices';
import { parseCalProfile } from '../src/calibration/calprofile';
import { runCalibration, splitCapture, type Rig, type RunInput } from '../src/calibration/runner';
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
        if (!match(e as never)) throw new Error('no sweep of that generation');
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
  const f = fake({ loaded: ['a/b', 'c/d'] });
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
  assert.deepEqual(calls, [['machine', 1, 'a/b']]);
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
  assert.deepEqual(f.log.slice(0, 2), ['midi 144,60,100', 'drained']);
});

test('the counters are stopped when a capture fails, and a capture is read after they stop', async () => {
  const f = fake();
  f.rig.stats.clear = async () => {
    throw new Error('clear failed');
  };
  await assert.rejects(run(f.rig, base + `[[case]]\nid = "stock"\nsteps = "hold"\n`), /clear failed/);
  assert.ok(f.log.includes('stats.stop'));
});

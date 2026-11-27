import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, type Clock, type DkSession } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { runCalibrationOnTarget, deviceOfInfo } from '../src/profile/calibration-run';
import { DEFAULT_STABLE_BLOCKS, sessionSetup, untilBootStable } from '../src/calibration/rigsetup';
import { loadCalibrationFiles, type RunEvent } from '../src/profile/calibration-view';

const files = loadCalibrationFiles({
  profiles: {
    'p/heavy.toml': `schema = "dk.calprofile/1"\nname = "heavy"\nwarmup_s = 1\ncapture_s = 2\nrepeats = 2\nteardown = { steps = "off" }\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
  },
  steps: {
    'hold.toml': `schema = "dk.steps/1"\nname = "hold"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n`,
    'off.toml': `schema = "dk.steps/1"\nname = "off"\n[[step]]\naction = "midi.note_off"\ntrack = 1\nkey = 60\n`,
  },
  cases: {},
});

function fake(opts: { hangOnPause?: number; fwId?: string | null } = {}) {
  const sent: string[] = [];
  const calls: string[] = [];
  let pauses = 0;
  const ac = new AbortController();
  const clock: Clock = {
    now: async () => 0,
    pause: async () => {
      pauses++;
      if (pauses === opts.hangOnPause) {
        setTimeout(() => ac.abort(), 0);
        await new Promise(() => {}); // the emulator never answers: Stop must still end the wait
      }
    },
  };
  const session = {
    clock,
    info: async () => ({
      port: 'dn2-1.12',
      runtime: '0.9',
      log_sink: true,
      ...(opts.fwId === null ? {} : { fw_id: opts.fwId ?? 'ab12' }),
    }),
    modules: async () => [],
    // no debug log: the event reader cannot start, the run goes on without it
    logPage: async () => {
      throw new Error('no log');
    },
    logConfig: async () => {
      throw new Error('no log');
    },
    profileState: async (op: string) => void calls.push(`state ${op}`),
    profileAll: async () => ({ hz: 1, elapsedTicks: 0n, spans: [] }),
    // a debug module without PROFILE_DSP: spans only
    profileDsp: async () => {
      throw new DkError({ error: 'unknown command', kind: 'device', status: 'E_PROTO' });
    },
  } as unknown as DkSession;
  return {
    deps: {
      session,
      emulator: true,
      sink: { send: async (b: Uint8Array) => void sent.push(Array.from(b).join(',')) },
      call: async <T>() => ({ queued: 0 }) as T,
      files,
    },
    ac,
    sent,
    calls,
  };
}

test('a run plays the profile through the shared runner and keeps every capture', async () => {
  const f = fake();
  const events: RunEvent[] = [];
  const seen: string[] = [];
  const out = await runCalibrationOnTarget(f.deps, {
    profile: 'heavy',
    onEvent: (e) => events.push(e),
    onCapture: (c) => seen.push(c.file),
  });
  assert.deepEqual([out.stopped, out.error, out.skipped], [false, undefined, []]);
  assert.deepEqual(seen, ['stock-1.profile.json', 'stock-2.profile.json']);
  assert.equal(out.captures.length, 2);
  const run = (out.captures[0]!.profile as { run: Record<string, unknown> }).run;
  assert.deepEqual([run.target, run.firmware_sha256, run.case_id], ['dn2', 'ab12', 'stock']);
  assert.ok((run.id as string).startsWith('emu-'));
  assert.deepEqual(events.filter((e) => e.kind === 'capture').length, 2);
  assert.equal(f.sent.at(-1), '128,60,0'); // the teardown's note off
});

test('Stop ends a wait that never returns, plays the teardown and keeps the captures already made', async () => {
  // waits: warm-up of capture 1 (1), its sweep-free capture window (2), warm-up of capture 2 (3) hangs
  const f = fake({ hangOnPause: 3 });
  const out = await runCalibrationOnTarget(f.deps, { profile: 'heavy', signal: f.ac.signal });
  assert.equal(out.stopped, true);
  assert.equal(out.error, undefined);
  assert.equal(out.captures.length, 1);
  assert.equal(f.sent.at(-1), '128,60,0');
  assert.equal(f.calls.at(-1), 'state stop'); // counters do not keep running
});

test('a target without a firmware identity is refused with the runner reason', async () => {
  const f = fake({ fwId: null });
  const out = await runCalibrationOnTarget(f.deps, { profile: 'heavy' });
  assert.match(out.error ?? '', /firmware identity/);
  assert.equal(out.captures.length, 0);
});

test('the device comes from the INFO port and must be in the registry', () => {
  assert.equal(deviceOfInfo('dn2-1.12'), 'dn2');
  assert.throws(() => deviceOfInfo('zz9-1.0'), /no known device/);
});

test('the run says up front which cases it cannot play, and hands over its clock', async () => {
  const f = fake();
  const withKeys = loadCalibrationFiles({
    profiles: {
      'p/k.toml': `schema = "dk.calprofile/1"\nname = "k"\nwarmup_s = 0.1\ncapture_s = 0.1\nrepeats = 1\n[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "keys"\nsteps = "press"\n`,
    },
    steps: {
      'hold.toml': `schema = "dk.steps/1"\nname = "hold"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n`,
      'press.toml': `schema = "dk.steps/1"\nname = "press"\n[[step]]\naction = "key"\nkey = "play"\n`,
    },
    cases: {},
  });
  const events: RunEvent[] = [];
  let clock: (() => Promise<number>) | undefined;
  const out = await runCalibrationOnTarget(
    { ...f.deps, files: withKeys },
    { profile: 'k', onEvent: (e) => events.push(e), onClock: (c) => (clock = c) },
  );
  assert.deepEqual(
    events.find((e) => e.kind === 'plan'),
    { kind: 'plan', skipping: ['keys'] },
  );
  assert.equal(out.skipped.length, 1);
  assert.equal(await clock!(), 0);
});

test('with the debug log a machine step is a SETUP request for the machine type the runtime reports for the module, awaited on SETUP_DONE', async () => {
  const setups: [string, number, number][] = [];
  const f = fake();
  const withMachine = loadCalibrationFiles({
    profiles: {
      'p/m.toml': `schema = "dk.calprofile/1"\nname = "m"\nwarmup_s = 0\ncapture_s = 0.1\nrepeats = 1\n[[case]]\nid = "stock"\nsteps = "hold"\n[[case]]\nid = "mach"\nsteps = "mach"\n[[case]]\nid = "fx"\nsteps = "fx"\n`,
    },
    steps: {
      'hold.toml': `schema = "dk.steps/1"\nname = "hold"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n`,
      'mach.toml': `schema = "dk.steps/1"\nname = "mach"\n[[step]]\naction = "setup"\ntrack = 3\nmachine = "a/b"\n`,
      'fx.toml': `schema = "dk.steps/1"\nname = "fx"\n[[step]]\naction = "setup"\neffect = "a/fx"\n`,
    },
    cases: {},
  });
  const session = f.deps.session as unknown as Record<string, unknown>;
  session.modules = async () => [{ index: 0, name: 'a/b', types: [17], rows: 1 }];
  session.logPage = async () => [0, 0, []];
  session.logConfig = async () => ({ level: 2, modules: [] });
  session.setLogConfig = async () => ({});
  session.setup = async (what: string, track: number, type: number) => {
    setups.push([what, track, type]);
    return 7;
  };
  // the clock moves 1 s per reading, so the wait for SETUP_DONE (which this fake log never holds) ends at once
  let t = 0;
  (f.deps.session as unknown as { clock: Clock }).clock.now = async () => (t += 1000);
  const out = await runCalibrationOnTarget({ ...f.deps, files: withMachine }, { profile: 'm' });
  // the run starts by putting the first used track on the stock machine (0-based track 0, type 0) and awaits its SETUP_DONE, which this log never holds: the run refuses up front
  assert.deepEqual(setups, [['machine', 0, 0]]);
  assert.match(out.error ?? '', /stock machine before the first case.*SETUP_DONE/s);
  assert.deepEqual(
    out.skipped.map((x) => x.case),
    [],
  );
});

test('a boot that is not seen stable stops the run with a clear message, and one whose events say it is stable does not wait', async () => {
  let t = 0;
  const clock: Clock = { now: async () => t, pause: async (ms) => void (t += ms) };
  const noLog = {
    logPage: async () => Promise.reject(new Error('no log')),
    info: async () => Promise.reject(new Error('no log')),
    logConfig: async () => Promise.reject(new Error('no log')),
    setLogConfig: async () => ({}),
    modules: async () => [],
  };
  await assert.rejects(untilBootStable(noLog as never, clock), /not seen stable.*the run stops here/);
  // BOOT_START (stable_blocks 3000) at block 100 and a later event at block 3200: stable by the blocks, though STABLE itself was dropped from the ring
  const word = [0, 0, 0];
  const events = [
    { seq: 1, time: 100, kind: LOG_KIND.BOOT_START, flags: 2, args: [0x0a04, 1, 3000, 1, 1] },
    { seq: 2, time: 3200, kind: LOG_KIND.HEALTH, flags: 2, args: [...word, 0, 0] },
  ];
  const log = {
    logPage: async (from: number) => [1, 2, events.filter((e) => e.seq >= from)] as never,
    info: async () => ({ log_sink: true }),
    logConfig: async () => ({ level: 2, modules: [] }),
    setLogConfig: async () => ({}),
    modules: async () => [],
  };
  await untilBootStable(log as never, clock);
});

test('an idle long boot is stable at once: a probe event carries the block count now, with STABLE dropped from the ring; a young boot waits for STABLE', async () => {
  let t = 0;
  const clock: Clock = { now: async () => t, pause: async (ms) => void (t += ms) };
  const events: { seq: number; time: number; kind: number; flags: number; args: number[] }[] = [
    { seq: 1, time: 100, kind: LOG_KIND.BOOT_START, flags: 2, args: [0x0a04, 1, 3000, 1, 1] },
  ];
  let listed = 0;
  const make = (nowBlocks: number) => ({
    logPage: async (from: number) => [1, events.length, events.filter((e) => e.seq >= from)] as never,
    // a command the runtime does not keep quiet logs an event stamped with the block count now
    list: async () => {
      listed++;
      events.push({ seq: events.length + 1, time: nowBlocks, kind: LOG_KIND.UPLOAD, flags: 2, args: [0, 0, 0, 0, 0] });
      return [];
    },
    info: async () => ({ log_sink: true }),
    logConfig: async () => ({ level: 2, modules: [] }),
    setLogConfig: async () => ({}),
    modules: async () => [],
  });
  // the boot is 10 minutes old and nothing was logged since: no waiting
  await untilBootStable(make(100 + 900_000) as never, clock);
  assert.equal(listed, 1);
  assert.equal(t, 0);
  // the boot is 1 s old (1500 blocks of 3000): it waits, reports, and STABLE ends the wait
  events.length = 1;
  const waits: [number, number][] = [];
  const young = make(100 + 1500);
  const done = untilBootStable(young as never, clock, (w, of) => {
    waits.push([w, of]);
    if (waits.length === 2)
      events.push({ seq: events.length + 1, time: 100 + 3000, kind: LOG_KIND.STABLE, flags: 2, args: [0, 0, 0, 0, 0] });
  });
  await done;
  assert.deepEqual(
    waits.map((w) => w[1]),
    [2, 2],
  );
  assert.ok(t >= 2000);
});

test('BOOT_START dropped from the ring: the probe event alone decides against the default stable period', async () => {
  let t = 0;
  const clock: Clock = { now: async () => t, pause: async (ms) => void (t += ms) };
  const events: { seq: number; time: number; kind: number; flags: number; args: number[] }[] = [];
  const make = (nowBlocks: number) => ({
    logPage: async (from: number) => [5, 5 + events.length, events.filter((e) => e.seq >= from)] as never,
    list: async () => {
      events.push({ seq: 6, time: nowBlocks, kind: LOG_KIND.UPLOAD, flags: 2, args: [0, 0, 0, 0, 0] });
      return [];
    },
    info: async () => ({ log_sink: true }),
    logConfig: async () => ({ level: 2, modules: [] }),
    setLogConfig: async () => ({}),
    modules: async () => [],
  });
  await untilBootStable(make(DEFAULT_STABLE_BLOCKS + 1000) as never, clock);
  assert.equal(t, 0);
  events.length = 0;
  const waits: number[] = [];
  await untilBootStable(make(DEFAULT_STABLE_BLOCKS - 3000) as never, clock, (w) => {
    waits.push(w);
    if (waits.length === 1)
      events.push({ seq: 7, time: DEFAULT_STABLE_BLOCKS, kind: LOG_KIND.STABLE, flags: 2, args: [0, 0, 0, 0, 0] });
  });
  assert.ok(waits.length >= 1 && t >= 1000);
});

test('the default stable period is the runtime ABI constant', () => {
  const abi = readFileSync(new URL('../../../runtime/core/abi.toml', import.meta.url), 'utf8');
  const m = /name = "STABLE_BLOCKS"\s*\ntype = "u32"\s*\nvalue = (\d+)/.exec(abi);
  assert.equal(DEFAULT_STABLE_BLOCKS, Number(m![1]));
});

test('the machine step sets a number as the type and a module id as its first reported type; it restores nothing', async () => {
  const sent: [string, number, number][] = [];
  const session = {
    modules: async () => [{ name: 'a/b', types: [17] }],
    setup: async (what: string, track: number, type: number) => {
      sent.push([what, track, type]);
      return sent.length;
    },
  };
  const clock: Clock = { now: async () => 0, pause: async () => {} };
  const events = {
    until: async () => ({ fields: { seq: sent.length, status: 0 } }),
    tryUntil: async () => undefined,
  };
  const setup = sessionSetup(session as never, events as never, clock);
  await setup('machine', 3, 'a/b');
  await setup('machine', 3, 0);
  assert.deepEqual(sent, [
    ['machine', 2, 17],
    ['machine', 2, 0],
  ]);
});

test('a module case whose module has no build for the target is skipped with that reason and the rest of the profile runs', async () => {
  const f = fake();
  const withModules = loadCalibrationFiles({
    profiles: {
      'p/m.toml': `schema = "dk.calprofile/1"\nname = "m"\nwarmup_s = 0\ncapture_s = 0.1\nrepeats = 1\ndiscover = "modules"\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
    },
    steps: {
      'hold.toml': `schema = "dk.steps/1"\nname = "hold"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n`,
    },
    cases: {
      'x/calibration.toml': `schema = "dk.calcase/1"\nid = "only-other"\nmodule = "tom/other"\nsteps = "hold"\n`,
    },
  });
  const events: RunEvent[] = [];
  const out = await runCalibrationOnTarget(
    { ...f.deps, files: withModules, modules: { library: async () => [], install: async () => {} } },
    { profile: 'm', onEvent: (e) => events.push(e) },
  );
  const port = (await f.deps.session.info()).port;
  assert.deepEqual(out.skipped, [{ case: 'only-other', reason: `not built for ${port}: tom/other` }]);
  assert.equal(out.error, undefined);
  assert.ok(out.captures.length > 0, 'the stock case still ran');
  assert.ok(events.some((e) => e.kind === 'skip' && e.case === 'only-other'));
});

test('`module#n` sets the n-th machine type of the module in the order of the MODULES reply, `module` the first', async () => {
  const sent: number[] = [];
  const session = {
    modules: async () => [
      { name: 'a/other', types: [9] },
      { name: 'a/b', types: [17, 18, 19] },
    ],
    setup: async (_what: string, _track: number, type: number) => {
      sent.push(type);
      return sent.length;
    },
  };
  const clock: Clock = { now: async () => 0, pause: async () => {} };
  const events = {
    until: async () => ({ fields: { seq: sent.length, status: 0 } }),
    tryUntil: async () => undefined,
  };
  const setup = sessionSetup(session as never, events as never, clock);
  await setup('machine', 1, 'a/b');
  await setup('machine', 1, 'a/b#1');
  await setup('machine', 1, 'a/b#3');
  assert.deepEqual(sent, [17, 17, 19]);
  await assert.rejects(setup('machine', 1, 'a/b#4'), /3 machine type\(s\), no machine #4/);
  await assert.rejects(setup('machine', 1, 'a/b#0'), /counts from 1/);
  await assert.rejects(setup('machine', 1, 'a/missing#2'), /not loaded/);
});

test('the production setup refuses a run-time machine type number that no module has loaded, before SETUP', async () => {
  const sent: number[] = [];
  const session = {
    modules: async () => [{ name: 'a/b', types: [17] }],
    setup: async (_what: string, _track: number, type: number) => {
      sent.push(type);
      return sent.length;
    },
  };
  const clock: Clock = { now: async () => 0, pause: async () => {} };
  const events = {
    until: async () => ({ fields: { seq: sent.length, status: 0 } }),
    tryUntil: async () => undefined,
  };
  const setup = sessionSetup(session as never, events as never, clock);
  await setup('machine', 1, 17);
  await assert.rejects(setup('machine', 1, 20), (e: unknown) => (e as { status?: number }).status === -46);
  assert.deepEqual(sent, [17]);
});

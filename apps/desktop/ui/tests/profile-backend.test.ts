// The Profile pane's recorder (src/profile-backend.ts) over a fake session, a fake controller rpc and a fake library: what it reads, what Start,
// Stop, Reset and Export do on the emulator and on a device.
import assert from 'node:assert/strict';
import { describe, test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { CAP, LOG_KIND } from '@digi/dk-session/gen_proto';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import { ownerName } from '@digi/workbench-core/profile/profile-core.ts';
import type { LibraryModule } from '@digi/workbench-core/modules/dktransfer.ts';
import { cfView, createRecorder, isDebugMissing, type ProfileSession } from '../src/profile-backend.ts';

/** A device of the registry without a DSP (its profile has no sharc.profile). */
const noDsp = registry.devices.find((d) => d.capabilities.dsp !== 'sharc')!;
const HZ = 132_000_000;
const NOW = new Date('2026-11-15T12:34:56Z');

interface FakeState {
  caps: number | null;
  running: boolean;
  elapsed: bigint;
  missing: boolean;
  /** The port the session reports (default: a DSP device). */
  port?: string;
  /** PROFILE_STATE start answers E_PROTO (the debug module has no span clock). */
  noClock?: boolean;
  calls: string[];
  health?: { avg: number; max: number };
  imageId?: string;
  fwId?: string;
  polls?: ({ owner: number | 'runtime'; fn: number } | undefined)[];
}

function fakeSession(st: FakeState): ProfileSession {
  const span = (
    row: number,
    kind: 'module' | 'phase' | 'poll',
    index: number,
    calls: number,
    sum: bigint,
    max: number,
  ) => ({
    row,
    kind,
    index,
    calls,
    sumTicks: sum,
    maxTicks: max,
  });
  return {
    info: async () => ({ port: st.port ?? 'dn2-1.12', caps: st.caps, image_id: st.imageId, fw_id: st.fwId }),
    modules: async () => [
      { index: 0, name: 'test/saw' },
      { index: 1, name: 'someone/mystery' },
    ],
    capacity: async () => ({ DSP_COST: { limit: 92_796, used: 0, largest: 0 } }),
    profileState: async (op: 'get' | 'start' | 'stop' | 'reset') => {
      st.calls.push(`state:${op}`);
      if (st.noClock && op === 'start') throw new DkError({ error: 'no clock', kind: 'device', status: 'E_PROTO' });
      if (op === 'start') st.running = true;
      if (op === 'stop') st.running = false;
      if (op === 'reset') st.elapsed = 0n;
      return { running: st.running, hz: HZ, rows: 15, elapsedTicks: st.elapsed };
    },
    profileAll: async () => {
      if (st.missing) throw new DkError({ error: 'unknown command', kind: 'device', status: 'E_PROTO' });
      return {
        running: st.running,
        hz: HZ,
        elapsedTicks: st.elapsed,
        spans: [span(0, 'module', 0, 10, 1000n, 300), span(8, 'phase', 0, 5, 50n, 20), span(11, 'poll', 0, 0, 0n, 0)],
        polls: st.polls ?? [],
      };
    },
    logPage: async () => {
      const ev = (seq: number, kind: number, args: number[]) => ({ seq, time: 0, kind, flags: 0, args });
      const events = st.health
        ? [ev(1, LOG_KIND.HEALTH, [4 << 16, st.health.avg * 10, st.health.max * 10, 0, 0]), ev(2, 1, [0, 0, 0, 0, 0])]
        : [ev(2, 1, [0, 0, 0, 0, 0])];
      return [1, 2, events] as [number, number, typeof events];
    },
    peek: async () => new Uint8Array(),
  } as unknown as ProfileSession;
}

const library = async (): Promise<LibraryModule[]> =>
  [{ name: 'test/saw', group: 'Machines' }] as unknown as LibraryModule[];

function fakeRpc(calls: string[], answers: Record<string, unknown>, fail: string[] = []): EmuRpc {
  return {
    call: async <T>(method: string, params?: Record<string, unknown>) => {
      calls.push(params ? `${method} ${JSON.stringify(params)}` : method);
      if (fail.includes(method)) throw new Error('built without it');
      const a = answers[method];
      return (Array.isArray(a) ? a.shift() : a) as T;
    },
  };
}

const sharcAnswer = {
  clock: { source: 'emu.sharc.insn', unit: 'insn', sampling: 's' },
  blocks: 100,
  icount: 1,
  idle_skipped: 0,
  counted: { fast: 1, block: 0, interpreted: 0 },
  raw: [],
};
const cfAnswer = { pc_samples: [['100', 3]], paths: [] };

const state = (over: Partial<FakeState> = {}): FakeState => ({
  caps: 1 << CAP.PEEK,
  running: false,
  elapsed: 0n,
  missing: false,
  calls: [],
  ...over,
});

describe('read', () => {
  test('timestamps span counters before owner enrichment and skips the separate HEALTH read for the drawer', async () => {
    const session = fakeSession(state({ running: true }));
    let clock = 1000;
    const recorder = createRecorder({
      session: async () => session,
      now: () => new Date(clock),
      loadLibrary: async () => {
        clock += 6500;
        return [];
      },
    });
    let healthReads = 0;
    session.logPage = async () => {
      healthReads++;
      throw new Error('Unexpected HEALTH read');
    };
    const r = await recorder.read('spans');
    assert.ok(!r.noDebug);
    assert.equal(r.observedAt, 1000);
    assert.equal(clock, 7500);
    assert.equal(healthReads, 0);
  });
  test('a device without the PEEK capability, or one that refuses PROFILE_STATE, has no debug module', async () => {
    for (const st of [state({ caps: 0 }), state({ missing: true })]) {
      const r = await createRecorder({
        session: async () => fakeSession(st),
        loadLibrary: library,
        now: () => NOW,
      }).read();
      assert.equal(r.noDebug, true);
    }
    assert.equal(isDebugMissing(new DkError({ error: 'x', kind: 'device', status: 'E_PROTO' })), true);
    assert.equal(isDebugMissing(new DkError({ error: 'x', kind: 'device', status: 'E_FULL' })), false);
    assert.equal(isDebugMissing(new Error('x')), false);
  });

  test('the spans and the modules, typed by the library; a module the library lacks is other', async () => {
    const r = await createRecorder({
      session: async () => fakeSession(state({ running: true, elapsed: 5000n })),
      loadLibrary: library,
      now: () => NOW,
    }).read();
    assert.ok(!r.noDebug);
    if (r.noDebug) return;
    assert.equal(r.emulator, false);
    assert.equal(r.snap.running, true);
    assert.equal(r.snap.elapsedTicks, 5000n);
    assert.deepEqual(r.modules, [
      { index: 0, name: 'test/saw', kind: 'machine' },
      { index: 1, name: 'someone/mystery', kind: 'other' },
    ]);
  });

  test('poll slots are named by their module and, when the address map names the function, by it', async () => {
    const asked: number[] = [];
    const rec = createRecorder({
      session: async () =>
        fakeSession(
          state({
            running: true,
            elapsed: 5000n,
            polls: [{ owner: 1, fn: 0x47e10040 }, undefined, { owner: 'runtime', fn: 0x40101000 }],
          }),
        ),
      loadLibrary: library,
      prof: async () =>
        ({
          addrLookup: (req: { addrs: { addr: number }[] }) => {
            asked.push(...req.addrs.map((a) => a.addr));
            return {
              unloaded: [],
              results: req.addrs.map((a) => ({ text: a.addr === 0x47e10040 ? 'poll_fn' : null })),
            };
          },
        }) as never,
      names: { inputs: () => ({ maps: [], loads: { modules: [] } }), refresh: async () => {} },
      now: () => NOW,
    });
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (r.noDebug) return;
    assert.deepEqual(asked, [0x47e10040, 0x40101000], 'one lookup of the used slots');
    assert.deepEqual(r.snap.polls, [
      { owner: 1, fn: 0x47e10040, name: 'poll_fn' },
      undefined,
      { owner: 'runtime', fn: 0x40101000 },
    ]);
    assert.equal(ownerName({ kind: 'poll', index: 0 }, r.modules, r.snap.polls), 'someone/mystery poll: poll_fn');
    assert.equal(ownerName({ kind: 'poll', index: 2 }, r.modules, r.snap.polls), 'runtime poll');
  });

  test('a library that cannot be read leaves every module other', async () => {
    const r = await createRecorder({
      session: async () => fakeSession(state({ elapsed: 1n })),
      loadLibrary: async () => {
        throw new Error('no folder');
      },
    }).read();
    assert.ok(!r.noDebug && r.modules.every((m) => m.kind === 'other'));
  });

  test('a device reads the whole-block DSP load from the newest HEALTH event, an emulator does not', async () => {
    const dev = await createRecorder({
      session: async () => fakeSession(state({ elapsed: 1n, health: { avg: 21.3, max: 34 } })),
      loadLibrary: library,
    }).read();
    assert.ok(!dev.noDebug);
    if (!dev.noDebug) assert.deepEqual(dev.deviceDsp, { avgPct: 21.3, maxPct: 34 });
    const none = await createRecorder({ session: async () => fakeSession(state()), loadLibrary: library }).read();
    assert.ok(!none.noDebug && none.deviceDsp === undefined);
  });
});

describe('the emulator', () => {
  test('Start resets and starts the spans, then starts the ColdFire and DSP profiles the same way', async () => {
    const st = state();
    const rpcCalls: string[] = [];
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc(rpcCalls, { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
    });
    await rec.start();
    assert.deepEqual(st.calls, ['state:reset', 'state:start']);
    assert.deepEqual(rpcCalls, ['cf.profile {"enable":true}', 'sharc.profile {"enable":true}']);
  });

  test('the DSP view appears while recording, with the device headroom, and names no module without maps', async () => {
    const st = state({ elapsed: 100n });
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
    });
    await rec.start();
    st.elapsed = 100n;
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (r.noDebug) return;
    assert.equal(r.emulator, true);
    assert.equal(r.dsp?.headroom, 92_796);
    assert.equal(r.dsp?.blocks, 100);
    assert.deepEqual(r.dsp?.modules, []);
  });

  test('Stop keeps the instruction profiles for Export and switches every profile off', async () => {
    const st = state({ elapsed: 5000n });
    const rpcCalls: string[] = [];
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc(rpcCalls, {
        'cf.profile': { pc_samples: [['100', 10]], paths: [] },
        'sharc.profile': sharcAnswer,
      }),
      loadLibrary: library,
      now: () => NOW,
    });
    await rec.start();
    st.elapsed = 5000n;
    rpcCalls.length = 0;
    await rec.stop();
    assert.deepEqual(rpcCalls, [
      'cf.profile',
      'sharc.profile',
      'cf.profile {"enable":false}',
      'sharc.profile {"enable":false}',
    ]);
    rpcCalls.length = 0;
    const f = await rec.exportFile();
    assert.deepEqual(rpcCalls, [], 'the stopped recording is exported from what Stop kept');
    const doc = JSON.parse(f.text);
    assert.equal(f.name, 'emu-20261115T123456.dkprof.json');
    assert.equal(doc.schema, 'dk.profile/1');
    assert.equal(doc.run.target, 'dn2');
    assert.equal(doc.span_elapsed_ticks, 5000);
    assert.equal(doc.span_clock_hz, HZ);
    assert.deepEqual(
      doc.raw.map((r: { cpu: string; count: number }) => [r.cpu, r.count]),
      [['cf', 10]],
      'cf.profile is what the daemon counted since Start',
    );
    assert.ok(doc.metrics.some((m: { name: string }) => m.name === 'sharc.blocks'));
    assert.deepEqual(
      doc.spans.map((s: { name: string; owner: number }) => [s.owner, s.name]),
      [
        [0, 'test/saw'],
        [0x100, 'early'],
        [0x200, 'poll 0'],
      ],
    );
  });

  test('a daemon built without the profile features still records the spans and says so', async () => {
    const st = state({ running: true, elapsed: 10n });
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], {}, ['cf.profile', 'sharc.profile']),
      loadLibrary: library,
    });
    await rec.start();
    assert.deepEqual(st.calls, ['state:reset', 'state:start']);
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (!r.noDebug) assert.match(r.note ?? '', /cf\.profile and sharc\.profile not available/);
  });

  test('the ColdFire view gives shares per path and the heaviest PCs, named by the address lookup, else bare', async () => {
    const st = state({ running: true, elapsed: 10n });
    const asked: number[] = [];
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], {
        'cf.profile': {
          pc_samples: [
            ['0x40101000', 30],
            ['0x40200000', 10],
            ['0x40300000', 60],
          ],
          paths: [
            { path: 'run_fast', instructions: 900, calls: 5, host_ns: 600 },
            { path: 'idle_skip', instructions: 100, calls: 2, host_ns: 200 },
          ],
        },
        'sharc.profile': sharcAnswer,
      }),
      loadLibrary: library,
      prof: async () =>
        ({
          addrLookup: (req: { addrs: { addr: number }[] }) => {
            asked.push(...req.addrs.map((a) => a.addr));
            return { unloaded: [], results: req.addrs.map((a) => ({ text: a.addr === 0x40101000 ? 'tick' : null })) };
          },
        }) as never,
      names: { inputs: () => ({ maps: [], loads: { modules: [] } }), refresh: async () => {} },
    });
    await rec.start();
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (r.noDebug) return;
    const fast = r.cf?.paths.find((p) => p.path === 'run_fast');
    assert.ok(Math.abs(fast!.timeShare - 0.75) < 1e-9 && Math.abs(fast!.insnShare - 0.9) < 1e-9);
    assert.equal(r.cf?.paths.find((p) => p.path === 'ram_clear')?.timeShare, 0);
    assert.deepEqual(
      r.cf?.pcs.map((c) => [c.addr, c.name, c.share]),
      [
        [0x40300000, undefined, 0.6],
        [0x40101000, 'tick', 0.3],
        [0x40200000, undefined, 0.1],
      ],
    );
    assert.deepEqual(asked, [0x40300000, 0x40101000, 0x40200000], 'one lookup, heaviest first');
  });

  test('changing symbol sources refreshes live PCs and a failed refresh leaves their addresses raw', async () => {
    const st = state({ running: true, elapsed: 10n });
    let revision = 0;
    let loadedRevision = -1;
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
      prof: async () =>
        ({
          addrLookup: () => ({ results: [{ text: `tick_${loadedRevision}+0x4` }] }),
        }) as never,
      names: {
        inputs: () => ({ maps: [], loads: { modules: [] } }),
        needsRefresh: () => revision !== loadedRevision,
        refresh: async () => {
          if (revision === 2) throw new Error('cannot read maps');
          loadedRevision = revision;
        },
      },
    });
    await rec.start();
    const firstPc = async () => {
      const r = await rec.read();
      assert.ok(!r.noDebug);
      return r.cf?.pcs[0];
    };
    assert.equal((await firstPc())?.name, 'tick_0+0x4');
    revision = 1;
    assert.equal((await firstPc())?.name, 'tick_1+0x4');
    revision = 2;
    assert.deepEqual(await firstPc(), { addr: 100, share: 1, weight: 3 });
  });

  test('export binds firmware SHA to verified capture data across a later image switch', async () => {
    const hash = 'ab'.repeat(32);
    const st = state({ imageId: 'ff'.repeat(8), fwId: hash.slice(0, 16) });
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
      names: {
        inputs: () => undefined,
        imageHash: () => hash,
        refresh: async () => {},
      },
      now: () => NOW,
    });
    await rec.start();
    st.elapsed = 17n;
    assert.equal(JSON.parse((await rec.exportFile()).text).run.firmware_sha256, hash);
    await rec.stop();
    st.imageId = 'ee'.repeat(8);
    st.fwId = 'cd'.repeat(8);
    st.elapsed = 99n;
    const saved = JSON.parse((await rec.exportFile()).text);
    assert.equal(saved.run.firmware_sha256, hash);
    assert.equal(saved.span_elapsed_ticks, 17);
    assert.equal(saved.raw[0].count, 3);
    await rec.start();
    assert.equal(
      JSON.parse((await rec.exportFile()).text).run.firmware_sha256,
      undefined,
      'the ship map is for the old image',
    );
  });

  test('an emulator reload invalidates the captured firmware SHA even under the same reported image id', async () => {
    const hash = 'ab'.repeat(32);
    const st = state({ imageId: 'ff'.repeat(8), fwId: hash.slice(0, 16) });
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], { 'cf.profile': { ...cfAnswer, restarts: 1 }, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
      names: {
        inputs: () => ({ maps: [{ key: { image_sha256: hash } }], loads: { modules: [] } }),
        refresh: async () => {},
      },
    });
    await rec.start();
    await rec.stop();
    assert.equal(JSON.parse((await rec.exportFile()).text).run.firmware_sha256, undefined);
  });

  test('the ColdFire view keeps only the top rows and has no shares when nothing was counted', () => {
    const part = {
      clock: { source: '', unit: '', sampling: '' },
      metrics: [],
      loss: [],
      overhead: [],
      raw: Array.from({ length: 30 }, (_, i) => ({ cpu: 'cf', addr: i, count: i + 1, series: 'pc' })).concat([
        { cpu: 'cf', addr: 999, count: 5000, series: 'idle_pc' },
      ]),
    };
    const v = cfView(part, undefined, 10);
    assert.equal(v.pcs.length, 10);
    assert.deepEqual(
      v.pcs.map((c) => c.addr),
      [29, 28, 27, 26, 25, 24, 23, 22, 21, 20],
      'only the pc series, heaviest first',
    );
    assert.ok(v.paths.every((p) => p.timeShare === 0 && p.insnShare === 0));
  });

  test('a firmware reload during the recording is said in the note', async () => {
    const st = state({ running: true, elapsed: 10n });
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc([], {
        'cf.profile': { ...cfAnswer, restarts: 2 },
        'sharc.profile': sharcAnswer,
      }),
      loadLibrary: library,
    });
    await rec.start();
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (!r.noDebug) assert.match(r.note ?? '', /cf\.profile: the firmware reloaded 2 times/);
    if (!r.noDebug) assert.doesNotMatch(r.note ?? '', /sharc\.profile: the firmware reloaded/);
  });

  test('Stop switches every source off even when one fails, and the spans error still comes out', async () => {
    const st = state({ running: true, elapsed: 10n });
    const calls: string[] = [];
    const rpc: EmuRpc = {
      call: async <T>(method: string, params?: Record<string, unknown>) => {
        calls.push(params ? `${method} ${JSON.stringify(params)}` : method);
        if (method === 'cf.profile' && params?.enable === false) throw new Error('cf stop failed');
        return (method === 'cf.profile' ? cfAnswer : sharcAnswer) as T;
      },
    };
    const rec = createRecorder({ session: async () => fakeSession(st), rpc, loadLibrary: library });
    await rec.start();
    calls.length = 0;
    await rec.stop();
    assert.ok(calls.includes('cf.profile {"enable":false}') && calls.includes('sharc.profile {"enable":false}'));
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (!r.noDebug) assert.match(r.note ?? '', /cf\.profile stop: cf stop failed/);
  });

  test('a device without a DSP does not call sharc.profile and says there is no DSP', async () => {
    const st = state({ port: `${noDsp.id}-1.0`, elapsed: 10n });
    const rpcCalls: string[] = [];
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc(rpcCalls, { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
    });
    await rec.start();
    await rec.stop();
    assert.ok(!rpcCalls.some((c) => c.startsWith('sharc.profile')));
    st.running = true;
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (!r.noDebug) assert.match(r.note ?? '', /no DSP on this device/);
    if (!r.noDebug) assert.equal(r.noDsp, true);
    if (!r.noDebug) assert.doesNotMatch(r.note ?? '', /built without/);
  });

  test('"built without" is said only for a daemon that lacks the method; another error shows its own message', async () => {
    const st = state({ elapsed: 10n });
    const rpc: EmuRpc = {
      call: async <T>(method: string) => {
        if (method === 'sharc.profile') throw new Error('sharc is busy');
        return cfAnswer as T;
      },
    };
    const rec = createRecorder({ session: async () => fakeSession(st), rpc, loadLibrary: library });
    await rec.start();
    const r = await rec.read();
    assert.ok(!r.noDebug);
    if (r.noDebug) return;
    assert.match(r.note ?? '', /sharc\.profile: sharc is busy/);
    assert.doesNotMatch(r.note ?? '', /built without/);
  });

  test('Reset zeroes the span counters and restarts the instruction profiles', async () => {
    const st = state();
    const rpcCalls: string[] = [];
    const rec = createRecorder({
      session: async () => fakeSession(st),
      rpc: fakeRpc(rpcCalls, { 'cf.profile': cfAnswer, 'sharc.profile': sharcAnswer }),
      loadLibrary: library,
    });
    await rec.start();
    st.calls.length = 0;
    rpcCalls.length = 0;
    await rec.reset();
    assert.deepEqual(st.calls, ['state:reset']);
    assert.deepEqual(rpcCalls, ['cf.profile {"enable":true}', 'sharc.profile {"enable":true}']);
  });
});

describe('a device', () => {
  test('Start on a device without a profiling clock says so; without the debug module it says that', async () => {
    const noClock = createRecorder({
      session: async () => fakeSession(state({ port: `${noDsp.id}-1.0`, noClock: true })),
      loadLibrary: library,
    });
    await assert.rejects(
      noClock.start(),
      new RegExp(`no profiling clock on this device \\(devices/${noDsp.stem}\\.toml`),
    );
    const noModule = createRecorder({
      session: async () => fakeSession(state({ caps: 0, noClock: true })),
      loadLibrary: library,
    });
    await assert.rejects(noModule.start(), /debug module not loaded/);
  });

  test('Start and Stop are the spans only, and Export writes the spans', async () => {
    const st = state({ elapsed: 777n });
    const rec = createRecorder({ session: async () => fakeSession(st), loadLibrary: library, now: () => NOW });
    await rec.start();
    st.elapsed = 777n; // the window grows while it records
    await rec.stop();
    assert.deepEqual(st.calls, ['state:reset', 'state:start', 'state:stop']);
    const doc = JSON.parse((await rec.exportFile()).text);
    assert.equal(doc.clock.source, 'runtime.span');
    assert.deepEqual(doc.raw, []);
    assert.equal(doc.span_elapsed_ticks, 777);
    assert.match(doc.run.id, /^dev-/);
  });
});

import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { Clock, DkSession } from '@digi/dk-session';
import { runCalibrationOnTarget, deviceOfInfo } from '../src/profile/calibration-run';
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
      throw new Error('E_PROTO');
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

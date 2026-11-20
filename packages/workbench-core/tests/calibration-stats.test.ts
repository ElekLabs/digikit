import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { sessionStatsSource, type StatsSession } from '../src/calibration/stats';

const sweep = { on: true, generation: 4, counts: { sweeps: 0, gaps: 0, ignored: 0 } };

function session(profileDsp: StatsSession['profileDsp']) {
  const log: string[] = [];
  const s: StatsSession = {
    profileState: async (op) => void log.push(`state ${op}`),
    profileAll: async () => ({ hz: 1, elapsedTicks: 1n, spans: [] }),
    profileDsp: async (op) => {
      log.push(`dsp ${op ?? 'get'}`);
      return profileDsp(op);
    },
  };
  return { s, log };
}

test('only the runtime answer for an unknown command means no DSP stats', async () => {
  const unknown = session(async () => {
    throw new DkError({ error: 'no such command', kind: 'device', status: 'E_PROTO' });
  });
  const source = sessionStatsSource(unknown.s, () => 'x');
  assert.deepEqual(await source.capabilities(), { dsp: false, spans: true });
  // spans-only: the DSP command is never sent again
  await source.start();
  assert.equal(unknown.log.filter((l) => l.startsWith('dsp')).length, 1);
  const ok = session(async () => sweep);
  assert.equal((await sessionStatsSource(ok.s, () => 'x').capabilities()).dsp, true);
});

test('a timeout, a busy channel or a link failure is not "no DSP stats": it throws', async () => {
  for (const e of [
    new DkError({ error: 'timed out', kind: 'timeout' }),
    new DkError({ error: 'busy', kind: 'device', status: 'E_BUSY' }),
    new Error('link lost'),
  ]) {
    const s = session(async () => {
      throw e;
    });
    await assert.rejects(sessionStatsSource(s.s, () => 'x').capabilities(), (got) => got === e);
  }
});

test('start stops counters an earlier run left on before it counts from zero', async () => {
  const { s, log } = session(async () => sweep);
  await sessionStatsSource(s, () => 'x').start();
  assert.deepEqual(log, ['state stop', 'dsp get', 'dsp stop', 'state reset', 'state start', 'dsp clear', 'dsp start']);
});

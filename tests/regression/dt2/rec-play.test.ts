import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { golden } from '@digi/test-support';
import { armFreeRegionGuard, expectNoGuardHits } from '../free-region-guard.ts';
import { withSession } from '../session-handle.ts';

// Replays tests/scenarios/dt2.json "dt2-rec-play" (exact_events: events land at their at_instr). The reference is
// `coupled-run --dsp zero --trace` on the same scenario; the expected hashes live in tests/regression/goldens/dt2-1.17.json.
const GOLDENS = path.join(import.meta.dirname, '..', 'goldens', 'dt2-1.17.json');
const TOTAL = 792_000_000; // 6 s
const MAX_STEP = 200_000_000;

const h = withSession({ device: 'dt2', firmware: '1.17', dsp: 'zero', restart: false });

describe('dt2 rec-play', () => {
  test('replay to 6 s', async () => {
    const { session } = h;
    const c = session.client;
    // Restart without the 300 ms settle: at_instr counts from the restored ready point, as in coupled-run.
    await c.restart(true);
    await armFreeRegionGuard(session);

    const scenario = (await import('../../scenarios/dt2.json', { with: { type: 'json' } })).default as {
      scenarios: { name: string; events: { at_instr: number }[] }[];
    };
    const events = scenario.scenarios.find((s) => s.name === 'dt2-rec-play')!.events;
    await c.inputSchedule(events);

    const points = [...new Set(events.map((e) => e.at_instr))].sort((a, b) => a - b);
    const seen: string[] = [];
    let at = 0;
    const advance = async (to: number) => {
      while (at < to) {
        const n = Math.min(to - at, MAX_STEP);
        await c.step(n, { timeout_wall_s: 300 });
        at += n;
      }
    };
    for (const p of points) {
      await advance(p);
      const hash = await session.frameHash();
      if (!seen.includes(hash)) seen.push(hash);
    }
    await advance(TOTAL);
    const final = await session.frameHash();

    await session.expectNoFault('rec-play');
    await expectNoGuardHits(session);
    golden(GOLDENS, 'rec-play.final', final);
    golden(GOLDENS, 'rec-play.event_frames', seen);
    expect(seen.length).toBeGreaterThan(0);
  });
});

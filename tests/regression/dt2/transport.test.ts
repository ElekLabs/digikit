import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { golden } from '@digi/test-support';
import { armFreeRegionGuard, expectNoGuardHits } from '../free-region-guard.ts';
import { withSession } from '../session-handle.ts';

const GOLDENS = path.join(import.meta.dirname, '..', 'goldens', 'dt2-1.17.json');

const h = withSession({ device: 'dt2', firmware: '1.17', dsp: 'zero' });

describe('dt2 transport', () => {
  test('PLAY, 1 s, STOP', async () => {
    const { session } = h;
    await armFreeRegionGuard(session);
    const state = async () => ({ frame: await session.frameHash(), leds: (await session.leds()).leds });

    const before = await state();
    await session.press('PLAY');
    await session.wait(1000);
    const playing = await state();
    await session.press('STOP');
    await session.wait(300);
    const stopped = await state();

    await session.expectNoFault('transport');
    await expectNoGuardHits(session);
    expect(playing.leds).not.toBe(before.leds);
    golden(GOLDENS, 'transport', { before, playing, stopped });
  });
});

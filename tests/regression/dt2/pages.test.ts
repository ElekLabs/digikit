import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { golden } from '@digi/test-support';
import { armFreeRegionGuard, expectNoGuardHits } from '../free-region-guard.ts';
import { withSession } from '../session-handle.ts';

const GOLDENS = path.join(import.meta.dirname, '..', 'goldens', 'dt2-1.17.json');
const PAGES = ['SRC', 'FLTR', 'AMP', 'FX', 'MOD'];

const h = withSession({ device: 'dt2', firmware: '1.17', dsp: 'zero' });

describe('dt2 parameter pages', () => {
  test.each(PAGES)('page %s', async (page) => {
    const { session } = h;
    await armFreeRegionGuard(session);
    const main = await session.frameHash();
    await session.press(page);
    await session.wait(300);
    const hash = await session.frameHash();

    await session.expectNoFault(page);
    await expectNoGuardHits(session);
    expect(hash).not.toBe(main);
    golden(GOLDENS, `pages.${page}`, hash);
  });
});

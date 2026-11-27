import { expect, it } from 'vitest';
import { startAtOnce } from '../resources.mjs';

it('starts coupled tasks first within the core budget', () => {
  expect(startAtOnce({ sharc: 57, zero: 9 }, 12, 12)).toEqual({ sharc: 4, zero: 0 });
  expect(startAtOnce({ sharc: 2, zero: 20 }, 12, 12)).toEqual({ sharc: 2, zero: 6 });
  expect(startAtOnce({ sharc: 0, zero: 20 }, 12, 12)).toEqual({ sharc: 0, zero: 12 });
});

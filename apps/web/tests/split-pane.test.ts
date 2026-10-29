// The pure logic of the split pane (apps/web/src/transfer/split-core.ts).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { HANDLE_PX, PANE_MIN_PX, SPLIT_DEFAULT, clampSplit, parseSplit, splitAt } from '../src/transfer/split-core.ts';

test('a saved position is used when it is a fraction, else the default', () => {
  assert.equal(parseSplit('0.4'), 0.4);
  for (const bad of [null, undefined, '', 'x', '0', '1', '-0.2', '1.5', 'NaN', 'Infinity'])
    assert.equal(parseSplit(bad), SPLIT_DEFAULT, String(bad));
});

test('both panes keep their minimum width', () => {
  const width = 1000;
  const room = width - HANDLE_PX;
  assert.equal(clampSplit(0.5, width), 0.5);
  assert.ok(Math.abs(clampSplit(0, width) * room - PANE_MIN_PX) < 1e-9);
  assert.ok(Math.abs((1 - clampSplit(1, width)) * room - PANE_MIN_PX) < 1e-9);
});

test('a container too narrow for two panes gives the middle', () => {
  assert.equal(clampSplit(0.9, 400), 0.5);
  assert.equal(clampSplit(0.9, 0), SPLIT_DEFAULT);
});

test('the pointer centres the divider and is clamped', () => {
  // container at x=100, 1000 wide: the pointer in the middle of the divider's area gives the matching fraction
  const room = 1000 - HANDLE_PX;
  const x = 100 + HANDLE_PX / 2 + 0.5 * room;
  assert.ok(Math.abs(splitAt(x, 100, 1000) - 0.5) < 1e-9);
  assert.equal(splitAt(-500, 100, 1000), clampSplit(0, 1000));
  assert.equal(splitAt(5000, 100, 1000), clampSplit(1, 1000));
});

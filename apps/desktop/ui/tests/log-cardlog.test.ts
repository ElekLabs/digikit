// The Card log button of the Log drawer: what it shows for each state the debug module reports (apps/desktop/ui/src/log-cardlog-core.ts).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { CardLogState } from '@digi/dk-session';
import { NO_CARD_LOG, cardLogView } from '../src/log-cardlog-core.ts';

const state = (over: Partial<CardLogState>): CardLogState => ({
  on: true,
  file: 'b',
  state: 'on',
  writes: 12,
  lastStatus: 0,
  msSinceWrite: 3000,
  ...over,
});

test('the title and the classes follow the state', () => {
  const on = cardLogView(state({}), '');
  assert.equal(on.title, 'Writing to cardlog-b.bin: 12 writes, last 3 s ago');
  assert.deepEqual([on.pressed, on.dot, on.warn, on.disabled], [true, true, false, false]);
  assert.equal(
    cardLogView(state({ writes: 0, msSinceWrite: null }), '').title,
    'Writing to cardlog-b.bin: 0 writes, no write yet',
  );
  const off = cardLogView(state({ on: false, state: 'off' }), '');
  assert.match(off.title, /^Write the device log to the card/);
  assert.deepEqual([off.pressed, off.dot], [false, false]);
  const full = cardLogView(state({ state: 'full' }), '');
  assert.equal(full.title, 'Card log stopped: store full');
  assert.deepEqual([full.pressed, full.warn], [true, true]);
  assert.equal(cardLogView(state({ state: 'error', lastStatus: -5 }), '').title, 'Card log stopped: error -5');
});

test('the button is off with the reason of the Level menu, or when there is no card log', () => {
  const why = 'The device is not followed: nothing answers for its log levels';
  assert.deepEqual([cardLogView(state({}), why).disabled, cardLogView(state({}), why).title], [true, why]);
  const none = cardLogView(null, '');
  assert.deepEqual([none.disabled, none.title], [true, NO_CARD_LOG]);
});

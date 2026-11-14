// The desktop pages' waiting, no-reply and cancellation messages.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { WAIT_QUIET_MS, failureLine, waitingLine } from '../src/transfer/wait-core.ts';

test('the lines for waiting, no reply and cancel', () => {
  assert.equal(waitingLine('MIDI: IAC Driver Bus 1'), 'Waiting for MIDI: IAC Driver Bus 1…');
  assert.ok(WAIT_QUIET_MS > 0);
  const timeout = new DkError({ error: 'no reply from the device', kind: 'timeout' });
  assert.match(failureLine(timeout, 'MIDI: IAC Driver Bus 1'), /^No reply from MIDI: IAC Driver Bus 1\./);
  assert.doesNotMatch(failureLine(timeout, 'x'), /Refresh/, 'there is no Refresh button to press');
  assert.equal(failureLine(new DkError({ error: 'cancelled', kind: 'cancelled' }), 'x'), 'Cancelled.');
  assert.equal(failureLine(new Error('no MIDI port is open'), 'x'), 'no MIDI port is open');
  assert.equal(failureLine('{"error":"disk full","kind":"device"}', 'x'), 'disk full');
});

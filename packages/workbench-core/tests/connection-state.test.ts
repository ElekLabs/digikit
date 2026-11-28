// The connection owner (packages/workbench-core/src/transfer/connection-state.ts): the grace timer.
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRoot } from 'solid-js';
import { createConnection, DISCONNECT_GRACE_MS } from '../src/transfer/connection-state.ts';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it('dispose while disconnected stops the grace timer', () => {
  const expired = vi.fn();
  const c = createRoot(() => createConnection({ deviceName: async () => 'x' }, undefined, { expired }));
  c.noteConnection({ type: 'lost', reason: 'no reply' });
  c.dispose();
  vi.advanceTimersByTime(DISCONNECT_GRACE_MS * 2);
  expect(expired).not.toHaveBeenCalled();
});

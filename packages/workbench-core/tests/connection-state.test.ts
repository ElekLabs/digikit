// The connection owner (packages/workbench-core/src/transfer/connection-state.ts): the grace timer.
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRoot } from 'solid-js';
import { createConnection, DISCONNECT_GRACE_MS } from '../src/transfer/connection-state.ts';
import { deviceProfiles, runtimeDeviceIds } from '../src/data/device-profiles.ts';

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

it('a lost optional request changes nothing and starts no re-read; a lost core request does both', () => {
  const lost = vi.fn();
  const c = createRoot(() => createConnection({ deviceName: async () => 'x' }, undefined, { lost }));
  c.noteConnection({
    type: 'answered',
    device: { id: runtimeDeviceIds[0]!, name: deviceProfiles[runtimeDeviceIds[0]!].name },
  });
  c.noteConnection({ type: 'lost', reason: 'no reply', optional: true });
  expect(c.connection().state).toBe('connected');
  expect(lost).not.toHaveBeenCalled();
  c.noteConnection({ type: 'lost', reason: 'no reply' });
  expect(c.connection().state).toBe('disconnected');
  expect(lost).toHaveBeenCalledOnce();
  c.dispose();
});

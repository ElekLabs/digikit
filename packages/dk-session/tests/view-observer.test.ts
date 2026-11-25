import { ViewObserverUnsupported, withViewObserver, type ViewObserverState } from '@digi/dk-session';
import { expect, test, vi } from 'vitest';

function fake() {
  let ms = 0;
  let state: ViewObserverState = { supported: true, enabled: false, generation: 0 };
  const clock = {
    now: async () => ms,
    pause: async (delay: number) => {
      ms += delay;
      if (state.enabled) state.snapshot = { object: 0x1234, classId: 0x40000020 };
    },
  };
  const events = { listen: vi.fn(async () => {}) };
  const info = vi.fn(async () => ({ port: 'test-profile', log_sink: true }));
  const log = { level: 2, modules: Array<number>(8).fill(0xff), debug: true };
  const logConfig = vi.fn(async () => log);
  const setLogConfig = vi.fn(async (_level: number, _modules: number[]) => log);
  const viewObserver = vi.fn(async () => state);
  const setViewObserver = vi.fn(async (enabled: boolean) => {
    state = { supported: true, enabled, generation: enabled ? 7 : 0 };
    return state;
  });
  const session = { clock, info, logConfig, setLogConfig, viewObserver, setViewObserver };
  return { session, events, log };
}

test('unsupported firmware refuses the scenario before listening or changing anything', async () => {
  const { session, events } = fake();
  const run = vi.fn(async () => {});
  session.viewObserver.mockResolvedValue({ supported: false, enabled: false, generation: 0 });
  const result = withViewObserver(session, events, run, { target: 'Syntakt OS 1.42' });
  await expect(result).rejects.toBeInstanceOf(ViewObserverUnsupported);
  await expect(result).rejects.toThrow('Syntakt OS 1.42 does not support ui.view');
  expect(run).not.toHaveBeenCalled();
  expect(events.listen).not.toHaveBeenCalled();
  expect(session.setViewObserver).not.toHaveBeenCalled();
  expect(session.setLogConfig).not.toHaveBeenCalled();
});

test('uses the existing listener, gets a baseline, and restores settings even when the scenario fails', async () => {
  const { session, events, log } = fake();
  const sentinel = new Error('scenario failed');
  await expect(
    withViewObserver(session, events, async (snapshot, generation) => {
      expect(events.listen).toHaveBeenCalledOnce();
      expect(snapshot).toEqual({ object: 0x1234, classId: 0x40000020 });
      expect(generation).toBe(7);
      throw sentinel;
    }),
  ).rejects.toBe(sentinel);
  expect(await session.viewObserver()).toEqual({ supported: true, enabled: false, generation: 0 });
  expect(session.setLogConfig).toHaveBeenCalledWith(log.level, log.modules);
});

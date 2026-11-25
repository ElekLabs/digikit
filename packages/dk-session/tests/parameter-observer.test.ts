import {
  matchesParameterChange,
  ParameterObserverUnsupported,
  resolveParameterWatch,
  runtimeEvent,
  withParameterObserver,
} from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect, test, vi } from 'vitest';

const profile = {
  name: 'Profile with mapped stock firmware',
  parameterWatches: {
    '1.17': { sampleSlot: { scope: 'track', word: 28 }, 'delay.feedback': { scope: 'kit', word: 35 } },
  },
} as const;
const slot = resolveParameterWatch(profile, '1.17', 'sampleSlot', { scope: 'track', track: 1 });

test('kind and scope require a mapping for the exact firmware', () => {
  expect(slot).toEqual({ scope: 'track', track: 1, kind: 'sampleSlot', word: 28 });
  expect(() => resolveParameterWatch(profile, '1.17', 'delay.feedback', { scope: 'track', track: 1 })).toThrow(
    ParameterObserverUnsupported,
  );
  expect(() => resolveParameterWatch(profile, '1.16', 'sampleSlot', { scope: 'track', track: 1 })).toThrow(
    ParameterObserverUnsupported,
  );
});

test('parameter events match the requested instance and generation', () => {
  const change = (key: number, generation: number) =>
    runtimeEvent({
      seq: 4,
      time: 1,
      kind: LOG_KIND.VALUE_CHANGED,
      flags: 6,
      args: [1, key, 0xffffffff, 2, generation],
    });
  const matches = (event: ReturnType<typeof change>) =>
    matchesParameterChange(event, slot, 7, (value, old) => value === 2 && old === -1);
  expect(matches(change(28, 7))).toBe(true);
  expect(matches(change((1 << 8) | 28, 7))).toBe(false);
  expect(matches(change(28, 6))).toBe(false);
});

function fake() {
  const config = { level: 2, modules: Array<number>(8).fill(0xff), debug: true };
  const session = {
    info: async () => ({ port: 'device', log_sink: true }),
    probeParameter: vi.fn(async () => ({ supported: true, generation: 0 })),
    startParameterObserver: vi.fn(async () => ({ supported: true, generation: 9 })),
    parameterObserver: vi.fn(async () => ({ supported: true, generation: 9, value: 0 })),
    stopParameterObserver: vi.fn(async (_generation: number) => {}),
    logConfig: vi.fn(async () => config),
    setLogConfig: vi.fn(async (_level: number, _modules: number[]) => config),
    clock: { now: async () => 0, pause: async () => {} },
  };
  return { session, events: { listen: vi.fn(async () => {}) }, config };
}

test('missing firmware support fails before any scenario input or log changes', async () => {
  const { session, events } = fake();
  session.probeParameter.mockResolvedValue({ supported: false, generation: 0 });
  const input = vi.fn(async () => {});
  await expect(withParameterObserver(session, events, slot, input)).rejects.toBeInstanceOf(
    ParameterObserverUnsupported,
  );
  expect(input).not.toHaveBeenCalled();
  expect(events.listen).not.toHaveBeenCalled();
  expect(session.startParameterObserver).not.toHaveBeenCalled();
});

test('a zero baseline is ready, and failure stops only that generation and restores levels', async () => {
  const { session, events, config } = fake();
  const error = new Error('scenario failed');
  await expect(
    withParameterObserver(session, events, slot, async (value, generation) => {
      expect(value).toBe(0);
      expect(generation).toBe(9);
      throw error;
    }),
  ).rejects.toBe(error);
  expect(session.stopParameterObserver).toHaveBeenCalledWith(9);
  expect(session.setLogConfig).toHaveBeenCalledWith(config.level, config.modules);
});

import type { DkInfoFull } from '@digi/dk-session';
import { expect, it } from 'vitest';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { latestDeviceHealth, readDeviceHealth, percent } from '../src/device-health';

it('finds the newest retained health sample even when the last page contains only other events', async () => {
  const events = Array.from({ length: 7 }, (_, i) => ({
    seq: i + 1,
    time: 120 + i,
    kind: i === 0 || i === 3 ? LOG_KIND.HEALTH : LOG_KIND.BOOT_START,
    flags: 0,
    args: [2 << 16, 1000, 1000, 0, 0],
  }));
  const health = await latestDeviceHealth({
    logPage: async (from) => [1, 7, events.filter((e) => e.seq >= from).slice(0, 2)],
  });
  expect(health).toEqual({ seq: 4, blocks: 123, cpu: 0, dsp: null, peak: null });
  expect(percent(health?.cpu)).toBe('0%');
  expect(percent(health?.dsp)).toBe('—');
});

it('stops before the next LOG request when the target job takes the port', async () => {
  const controller = new AbortController();
  const requests: number[] = [];
  const reading = latestDeviceHealth(
    {
      logPage: async (from) => {
        requests.push(from);
        controller.abort();
        return [1, 10, [{ seq: 1, time: 0, flags: 0, kind: LOG_KIND.BOOT_START, args: [] }]];
      },
    },
    controller.signal,
  );
  await expect(reading).rejects.toThrow();
  expect(requests).toEqual([0]);
});

it('does not start a LOG read when a job starts while INFO is pending', async () => {
  const stop = new AbortController();
  let finish!: (info: DkInfoFull) => void;
  const info = new Promise<DkInfoFull>((resolve) => {
    finish = resolve;
  });
  let logs = 0;
  const result = readDeviceHealth(
    {
      info: () => info,
      logPage: async () => {
        logs++;
        return [0, 0, []];
      },
    },
    stop.signal,
  );
  stop.abort();
  finish({ caps: 1 } as DkInfoFull);
  await expect(result).rejects.toThrow();
  expect(logs).toBe(0);
});

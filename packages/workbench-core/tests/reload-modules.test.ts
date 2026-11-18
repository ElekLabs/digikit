import { expect, test } from 'vitest';
import { DkError, type DkSession } from '@digi/dk-session';
import { reloadModules } from '../src/modules/dktransfer';

type Answer = number | Error;

/** A session whose `reloadAndWait` answers, per path, a result (0 is ok) or an error the device gave when it queued the reload. */
const fakeSession = (answers: Record<string, Answer>) => {
  const asked: string[] = [];
  const s = {
    info: async () => ({ runtime: 0x205, card_read_only: false }),
    reloadAndWait: async (path: string) => {
      asked.push(path);
      const a = answers[path]!;
      if (a instanceof Error) throw a;
      return a;
    },
  } as unknown as DkSession;
  return { s, asked };
};

test('reloadModules: a module that reloads is reported, one that the runtime refuses or does not have is left to the restart', async () => {
  const notLoaded = new DkError({ error: 'no such module', kind: 'device', status: 'E_NOT_FOUND' });
  const { s, asked } = fakeSession({ 'm/a.dkm': 0, 'm/b.dkm': -83, 'm/c.dkm': notLoaded });
  const events: { event: string }[] = [];
  const out = await reloadModules(s, ['m/a.dkm', 'm/b.dkm', 'm/c.dkm'], (e) => events.push(e as { event: string }));
  expect(asked).toEqual(['m/a.dkm', 'm/b.dkm', 'm/c.dkm']);
  expect(out.reloaded).toEqual(['m/a.dkm']);
  expect(out.fell_back).toEqual([
    { path: 'm/b.dkm', result: -83 },
    { path: 'm/c.dkm', result: 'E_NOT_FOUND' },
  ]);
  expect(events.map((e) => e.event)).toEqual([
    'reload',
    'reload',
    'reload-refused',
    'reload',
    'reload-refused',
    'done',
  ]);
});

test('reloadModules: another device error passes on (a store that cannot answer is no reason to boot)', async () => {
  const broken = new DkError({ error: 'the link is down', kind: 'transport' });
  const { s } = fakeSession({ 'm/a.dkm': broken });
  await expect(reloadModules(s, ['m/a.dkm'])).rejects.toThrow('the link is down');
});

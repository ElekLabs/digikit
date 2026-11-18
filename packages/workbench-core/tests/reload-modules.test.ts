import { expect, test } from 'vitest';
import { DkError, type DkSession } from '@digi/dk-session';
import { reloadModules } from '../src/modules/dktransfer';

type Answer = { result: number; unloaded: boolean } | Error;

/** A session whose `reloadAndWait` answers, per path, the job's end or an error the device gave when it queued the reload. */
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

test('reloadModules: reloaded, refused before the unload (the old module runs) and failed after it (not loaded) are three lists with numeric results', async () => {
  const notLoaded = new DkError({ error: 'no such module', kind: 'device', status: 'E_NOT_FOUND' });
  const { s, asked } = fakeSession({
    'm/a.dkm': { result: 0, unloaded: true },
    'm/b.dkm': { result: -83, unloaded: false },
    'm/c.dkm': notLoaded,
    'm/d.dkm': { result: -9, unloaded: true },
  });
  const events: { event: string }[] = [];
  const out = await reloadModules(s, ['m/a.dkm', 'm/b.dkm', 'm/c.dkm', 'm/d.dkm'], (e) =>
    events.push(e as { event: string }),
  );
  expect(asked).toEqual(['m/a.dkm', 'm/b.dkm', 'm/c.dkm', 'm/d.dkm']);
  expect(out.reloaded).toEqual(['m/a.dkm']);
  expect(out.refused).toEqual([
    { path: 'm/b.dkm', result: -83 },
    { path: 'm/c.dkm', result: -1 },
  ]);
  expect(out.failed).toEqual([{ path: 'm/d.dkm', result: -9 }]);
  expect(events.map((e) => e.event)).toEqual([
    'reload',
    'reload',
    'reload-refused',
    'reload',
    'reload-refused',
    'reload',
    'reload-failed',
    'done',
  ]);
});

test('reloadModules: another device error passes on (a store that cannot answer is no reason to boot)', async () => {
  const broken = new DkError({ error: 'the link is down', kind: 'transport' });
  const { s } = fakeSession({ 'm/a.dkm': broken });
  await expect(reloadModules(s, ['m/a.dkm'])).rejects.toThrow('the link is down');
});

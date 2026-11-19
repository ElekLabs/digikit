import { expect, test } from 'vitest';
import { DkError, type DkSession } from '@digi/dk-session';
import { liveModules, reloadModules } from '../src/modules/dktransfer';
import { eventText } from '../src/transfer/transfer-core';

type Answer = { result: number; unloaded: boolean } | Error;

/** A session whose `reloadAndWait` answers, per path, the job's end or an error the device gave when it queued the reload. */
const fakeSession = (answers: Record<string, Answer>) => {
  const asked: string[] = [];
  const answer = async (op: string, path: string) => {
    asked.push(op === 'reload' ? path : `${op} ${path}`);
    const a = answers[path]!;
    if (a instanceof Error) throw a;
    return a;
  };
  const s = {
    info: async () => ({ runtime: 0x205, card_read_only: false }),
    reloadAndWait: async (path: string) => answer('reload', path),
    loadAndWait: async (path: string) => answer('load', path),
    unloadAndWait: async (path: string) => answer('unload', path),
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

test('liveModules: a load and an unload sort their results the same way and name the operation in the events', async () => {
  const loaded = new DkError({ error: 'already loaded', kind: 'device', status: 'E_LOADED' });
  const { s, asked } = fakeSession({
    'm/a.dkm': { result: 0, unloaded: false },
    'm/b.dkm': { result: -83, unloaded: false },
    'm/c.dkm': loaded,
    'm/d.dkm': { result: -9, unloaded: true },
  });
  const events: { event: string }[] = [];
  const out = await liveModules(s, 'load', ['m/a.dkm', 'm/b.dkm', 'm/c.dkm', 'm/d.dkm'], (e) =>
    events.push(e as { event: string }),
  );
  expect(asked).toEqual(['load m/a.dkm', 'load m/b.dkm', 'load m/c.dkm', 'load m/d.dkm']);
  expect(out.done).toEqual(['m/a.dkm']);
  expect(out.refused).toEqual([
    { path: 'm/b.dkm', result: -83 },
    { path: 'm/c.dkm', result: -10 },
  ]);
  expect(out.failed).toEqual([{ path: 'm/d.dkm', result: -9 }]);
  expect(events.map((e) => e.event)).toEqual([
    'load',
    'load',
    'load-refused',
    'load',
    'load-refused',
    'load',
    'load-failed',
    'done',
  ]);
  expect(events.at(-1)).toMatchObject({ event: 'done', op: 'load', done: ['m/a.dkm'] });
  expect(eventText(events.at(-1) as never)).toBe('Loaded m/a.dkm');
  const gone = await liveModules(fakeSession({ 'm/a.dkm': { result: 0, unloaded: true } }).s, 'unload', ['m/a.dkm']);
  expect(gone).toEqual({ done: ['m/a.dkm'], refused: [], failed: [] });
});

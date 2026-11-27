import { describe, expect, it } from 'vitest';
import { createMemoryModuleClient, MemorySettingsStore, settingError } from '../src/index.ts';
import { schema } from './schema.ts';

describe('createMemoryModuleClient', () => {
  it('starts from initial display values', async () => {
    const s = createMemoryModuleClient(schema, { initial: { gain: 0.4, mode: 'Hard', name: 'ab' } }).settings(schema);
    expect(await s.get('gain')).toMatchObject({ value: 0.4, revision: 1, unset: false });
    expect(await s.get('mode')).toMatchObject({ value: 'Hard' });
    expect(await s.get('flag')).toMatchObject({ unset: true });
  });
  it('refuses an initial value that breaks the rule', () => {
    expect(() => createMemoryModuleClient(schema, { initial: { mode: 'Loud' } })).toThrow(/one of/);
    expect(() => createMemoryModuleClient(schema, { initial: { nope: 1 } })).toThrow(/Unknown/);
  });
  it('keeps values across clients that share a store and nothing across fresh ones', async () => {
    const store = new MemorySettingsStore(schema);
    const first = createMemoryModuleClient(schema, { store }).settings(schema);
    await first.get('name');
    await first.put('name', 'A');
    const again = createMemoryModuleClient(schema, { store }).settings(schema);
    expect(await again.get('name')).toMatchObject({ value: 'A', revision: 1 });
    expect(await createMemoryModuleClient(schema).settings(schema).get('name')).toMatchObject({ value: null });
  });
  it('fails every call with unavailable once the instance is gone', async () => {
    let live = true;
    const client = createMemoryModuleClient(schema, { current: () => live });
    const s = client.settings(schema);
    await s.get('name');
    live = false;
    expect(await s.put('name', 'late')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    expect(await s.get('name')).toMatchObject({ error: { code: 'unavailable' } });
    await expect(client.files.list()).rejects.toThrow(/closed/);
  });
  it('holds a call until released and lets a refusal through', async () => {
    let release = () => {};
    let mode: 'hold' | 'busy' | 'pass' = 'hold';
    const s = createMemoryModuleClient(schema, {
      behavior: () =>
        mode === 'hold'
          ? new Promise<void>((r) => (release = r))
          : mode === 'busy'
            ? settingError('busy', 'Full.')
            : undefined,
    }).settings(schema);
    let done = false;
    const pending = s.get('gain').then((r) => ((done = true), r));
    await Promise.resolve();
    expect(done).toBe(false);
    release();
    expect(await pending).toMatchObject({ ok: true });
    mode = 'busy';
    expect(await s.get('gain')).toEqual({ ok: false, error: { code: 'busy', message: 'Full.' } });
    mode = 'pass';
    expect(await s.get('gain')).toMatchObject({ ok: true });
  });
  it('lets the transport report an overlay scope error', async () => {
    const s = createMemoryModuleClient(schema, {
      behavior: ({ method }) =>
        method === 'put' ? settingError('overlay-scope', 'Change it on the device.') : undefined,
    }).settings(schema);
    await s.get('name');
    expect(await s.put('name', 'x')).toMatchObject({ error: { code: 'overlay-scope' } });
  });
  it('holds files', async () => {
    const { files } = createMemoryModuleClient(schema);
    const seen: number[][] = [];
    await files.send('a.dkbs', Uint8Array.of(1, 2), (sent, total) => seen.push([sent, total]));
    expect(seen).toEqual([[2, 2]]);
    expect(await files.list()).toEqual([{ name: 'a.dkbs', size: 2 }]);
    expect(await files.read('a.dkbs')).toEqual(Uint8Array.of(1, 2));
    await files.remove('a.dkbs');
    await expect(files.read('a.dkbs')).rejects.toThrow(/No such file/);
  });
});

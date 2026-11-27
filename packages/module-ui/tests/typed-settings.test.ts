import { describe, expect, expectTypeOf, it } from 'vitest';
import {
  createMemoryModuleChannel,
  createModuleClient,
  type ModuleClient,
  type SettingResult,
  type SettingsOf,
  type SettingsSchema,
  type TypedSettings,
} from '../src/index.ts';
import { schema } from './schema.ts';

describe('SettingsOf infers the value types of a generated schema', () => {
  it('maps each kind', () => {
    type V = SettingsOf<typeof schema>;
    expectTypeOf<V['gain']>().toEqualTypeOf<number>();
    expectTypeOf<V['flag']>().toEqualTypeOf<boolean>();
    expectTypeOf<V['mode']>().toEqualTypeOf<'Off' | 'Soft' | 'Hard'>();
    expectTypeOf<V['name']>().toEqualTypeOf<string | null>();
    expectTypeOf<V['anim']>().toEqualTypeOf<string | null>();
    expectTypeOf<keyof V>().toEqualTypeOf<'gain' | 'flag' | 'mode' | 'name' | 'anim'>();
  });
  it('types the calls of a client from the schema', async () => {
    const client: ModuleClient = createModuleClient(createMemoryModuleChannel(schema).channel);
    const s = client.settings(schema);
    expectTypeOf(s).toEqualTypeOf<TypedSettings<SettingsOf<typeof schema>>>();
    expectTypeOf(s.get('mode')).toEqualTypeOf<Promise<SettingResult<'Off' | 'Soft' | 'Hard'>>>();
    // @ts-expect-error an unknown id
    void s.get('nope');
    // @ts-expect-error a label that the choice does not have
    void s.put('mode', 'Loud');
    // @ts-expect-error a string for an integer
    void s.put('gain', '1');
    // @ts-expect-error null is a reset, not a put
    void s.put('name', null);
    const plain: SettingsSchema = schema; // the generated constant is a SettingsSchema
    expect(plain.settings).toHaveLength(5);
  });
});

describe('TypedSettings', () => {
  const open = () => createModuleClient(createMemoryModuleChannel(schema).channel).settings(schema);

  it('reads the default of an unset setting and null for unset text', async () => {
    const s = open();
    expect(await s.get('gain')).toEqual({ ok: true, value: 0, revision: 0, unset: true });
    expect(await s.get('mode')).toMatchObject({ value: 'Soft', unset: true });
    expect(await s.get('flag')).toMatchObject({ value: true, unset: true });
    expect(await s.get('name')).toMatchObject({ value: null, unset: true });
  });

  it('puts and reads back display values and bumps the revision', async () => {
    const s = open();
    await s.get('gain');
    expect(await s.put('gain', 0.6)).toEqual({ ok: true, value: 0.6, revision: 1, unset: false });
    expect(await s.put('gain', -1)).toMatchObject({ value: -1, revision: 2 });
    expect(await s.get('gain')).toMatchObject({ value: -1, revision: 2 });
  });

  it('refuses an invalid value without reaching the device and keeps the stored value', async () => {
    const calls: string[] = [];
    const s = createModuleClient(
      createMemoryModuleChannel(schema, { behavior: (c) => void calls.push(c.method) }).channel,
    ).settings(schema);
    await s.get('gain');
    const bad = await s.put('gain', 0.5); // off the step of 0.2
    expect(bad).toMatchObject({ ok: false, error: { code: 'invalid' } });
    expect(await s.put('gain', 11)).toMatchObject({ ok: false, error: { code: 'invalid' } });
    expect(calls).toEqual(['get']);
    expect(await s.get('gain')).toMatchObject({ value: 0, unset: true });
  });

  it('refuses a write before any read (not-read) and after another writer changed the setting', async () => {
    const client = createModuleClient(createMemoryModuleChannel(schema).channel);
    const a = client.settings(schema);
    const b = client.settings(schema);
    expect(await a.put('name', 'x')).toMatchObject({ error: { code: 'not-read' } });
    expect(await a.reset('name')).toMatchObject({ error: { code: 'not-read' } });
    await a.get('name');
    await b.get('name');
    expect(await b.put('name', 'b')).toMatchObject({ ok: true, revision: 1 });
    expect(await a.put('name', 'a')).toMatchObject({
      error: { code: 'conflict', message: expect.stringMatching(/changed/) },
    });
    expect(await a.reset('name')).toMatchObject({ error: { code: 'conflict' } });
    expect(await a.put('name', 'a', 1)).toMatchObject({ ok: true, value: 'a' });
  });

  it('keeps empty text distinct from unset and reset returns to unset', async () => {
    const s = open();
    await s.get('name');
    expect(await s.put('name', '')).toMatchObject({ value: '', unset: false });
    expect(await s.reset('name')).toMatchObject({ value: null, unset: true, revision: 2 });
  });

  it('rejects an unknown setting id as invalid', async () => {
    const s = open() as unknown as TypedSettings<Record<string, number>>;
    expect(await s.get('nope')).toMatchObject({ ok: false, error: { code: 'invalid' } });
  });
});

describe('a stored value that fails the setting rule', () => {
  it('reads as invalid-stored, keeping the revision for a later read', async () => {
    const bad = (code?: 'invalid-stored') => ({
      getSetting: async () =>
        code
          ? ({ ok: false, error: { code, message: 'x' } } as const)
          : ({ ok: true, value: Uint8Array.of(0, 0, 1, 0), revision: 1 } as const),
    });
    const { channel } = createMemoryModuleChannel(schema);
    const client = createModuleClient({ ...channel, ...bad() });
    expect(await client.settings(schema).get('gain')).toMatchObject({ ok: false, error: { code: 'invalid-stored' } });
    const refused = createModuleClient({ ...channel, ...bad('invalid-stored') });
    expect(await refused.settings(schema).get('gain')).toMatchObject({ ok: false, error: { code: 'invalid-stored' } });
  });
});

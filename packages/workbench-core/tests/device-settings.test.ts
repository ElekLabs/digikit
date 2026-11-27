import { describe, expect, it } from 'vitest';
import { DkError, type ModuleSettingsEndpoint } from '@digi/dk-session';
import { encodeDescriptor, type DescribedSchema } from '@digi/module-ui';
import { describeDeviceSettings, deviceSettings, settingErrorOf } from '../src/module-ui/device-settings.ts';

const schema: DescribedSchema = {
  name: 'Typed',
  version: 2,
  settings: [
    {
      id: 'gain',
      label: 'Gain',
      kind: 'integer',
      apply: 'immediate',
      min: 0,
      max: 100,
      step: 1,
      decimals: 2,
      unit: '',
      default: 0,
    },
    { id: 'name', label: 'Name', kind: 'text', apply: 'immediate', maxBytes: 8 },
  ],
};

function endpoint(over: Partial<ModuleSettingsEndpoint> = {}) {
  const sent: { field: number; value?: Uint8Array; revision: number }[] = [];
  const point: ModuleSettingsEndpoint = {
    describe: async () => encodeDescriptor(schema.name, schema),
    get: async () => ({ value: Uint8Array.of(0, 0, 0, 25), revision: 3 }),
    put: async (field, value, revision) => (sent.push({ field, value, revision }), { value, revision: revision + 1 }),
    reset: async (field, revision) => (sent.push({ field, revision }), { revision: revision + 1 }),
    ...over,
  };
  return { point, sent };
}

describe('deviceSettings', () => {
  it('reads the descriptor the device reports and refuses a broken one', async () => {
    expect(await describeDeviceSettings(endpoint().point)).toEqual(schema);
    await expect(describeDeviceSettings(endpoint({ describe: async () => Uint8Array.of(2, 1) }).point)).rejects.toThrow(
      /descriptor/,
    );
  });
  it('decodes stored bytes to display values and sends scaled bytes with the last revision and the field index', async () => {
    const { point, sent } = endpoint();
    const s = deviceSettings(schema, point);
    expect(await s.get('gain')).toEqual({ ok: true, value: 0.25, revision: 3, unset: false });
    expect(await s.put('gain', 0.75)).toMatchObject({ ok: true, value: 0.75, revision: 4 });
    expect(sent[0]).toEqual({ field: 0, value: Uint8Array.of(0, 0, 0, 75), revision: 3 });
    await s.get('name');
    expect(await s.reset('name')).toMatchObject({ ok: true, value: null, unset: true });
    expect(sent[1]).toMatchObject({ field: 1 });
  });
  it('refuses an invalid value before it reaches the device', async () => {
    const { point, sent } = endpoint();
    const s = deviceSettings(schema, point);
    await s.get('gain');
    expect(await s.put('gain', 1.5)).toMatchObject({ ok: false, error: { code: 'invalid' } });
    expect(sent).toEqual([]);
  });
  it('maps each device status to its typed error', () => {
    const status = (name: string) => settingErrorOf(new DkError({ error: 'x', kind: 'device', status: name })).code;
    expect(status('E_MODULE_UI_CONFLICT')).toBe('conflict');
    expect(status('E_MODULE_UI_SCOPE')).toBe('overlay-scope');
    expect(status('E_AGAIN')).toBe('busy');
    expect(status('E_SET_VALUE')).toBe('invalid');
    expect(status('E_NOFILE')).toBe('unavailable');
    expect(settingErrorOf(new Error('The module page has closed.'))).toEqual({
      code: 'unavailable',
      message: 'The module page has closed.',
    });
  });
  it('returns a thrown exchange as a typed error and a corrupt stored value as unavailable', async () => {
    const conflict = new DkError({ error: 'The setting changed.', kind: 'device', status: 'E_MODULE_UI_CONFLICT' });
    const { point } = endpoint({
      put: async () => {
        throw conflict;
      },
    });
    const s = deviceSettings(schema, point);
    await s.get('gain');
    expect(await s.put('gain', 0.5)).toEqual({
      ok: false,
      error: { code: 'conflict', message: 'The setting changed.' },
    });
    const bad = deviceSettings(
      schema,
      endpoint({ get: async () => ({ value: Uint8Array.of(0, 0, 1, 0), revision: 1 }) }).point,
    );
    expect(await bad.get('gain')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
  });
});

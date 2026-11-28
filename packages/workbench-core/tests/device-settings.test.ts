import { describe, expect, it } from 'vitest';
import { DkError, type ModuleSettingsEndpoint } from '@digi/dk-session';
import { createModuleClient, type DescribedSchema } from '@digi/module-ui';
import { createDeviceModuleChannel } from '../src/module-ui/device-module-channel.ts';
import { deviceQueryChannel } from '../src/module-ui/device-queries.ts';
import { deviceSettingsChannel, settingErrorOf } from '../src/module-ui/device-settings.ts';

// The descriptor the device reports for `schema` (the shape is checked against patchkit's bytes by the shared vectors).
const descriptor = Uint8Array.from(
  '0205547970656402046761696e044761696e040200000000000000006400000001020000000000046e616d65044e616d65080000'.match(
    /../g,
  )!,
  (h) => parseInt(h, 16),
);

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

const files = {
  list: async () => [],
  read: async () => new Uint8Array(),
  send: async () => {},
  remove: async () => {},
};
const settingsOf = (endpoint: ModuleSettingsEndpoint) => deviceSettingsChannel('Typed', async () => endpoint);
const deviceSettings = (s: DescribedSchema, endpoint: ModuleSettingsEndpoint) =>
  createModuleClient(
    createDeviceModuleChannel({ settings: settingsOf(endpoint), queries: deviceQueryChannel(), files }),
  ).settings(s);

function endpoint(over: Partial<ModuleSettingsEndpoint> = {}) {
  const sent: { field: number; value?: Uint8Array; revision: number }[] = [];
  const point: ModuleSettingsEndpoint = {
    describe: async () => descriptor,
    get: async () => ({ value: Uint8Array.of(0, 0, 0, 25), revision: 3 }),
    put: async (field, value, revision) => (sent.push({ field, value, revision }), { value, revision: revision + 1 }),
    reset: async (field, revision) => (sent.push({ field, revision }), { revision: revision + 1 }),
    ...over,
  };
  return { point, sent };
}

describe('deviceSettings', () => {
  it('reads the descriptor the device reports and refuses a broken one', async () => {
    expect(await settingsOf(endpoint().point).describeSettings()).toEqual(schema);
    await expect(
      settingsOf(endpoint({ describe: async () => Uint8Array.of(2, 1) }).point).describeSettings(),
    ).rejects.toThrow(/descriptor/);
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
  it('fetches the endpoint and the descriptor at the first request and again after a failure', async () => {
    let attempts = 0;
    const { point } = endpoint();
    const channel = deviceSettingsChannel('Typed', async () => {
      if (++attempts === 1) throw new Error('The selected target changed.');
      return point;
    });
    expect(await channel.getSetting('gain')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    expect(await channel.getSetting('gain')).toMatchObject({ ok: true, revision: 3 });
    expect(await channel.describeSettings()).toEqual(schema);
    expect(attempts).toBe(2);
  });
  it('answers a read the device refuses as an invalid stored value with invalid-stored, a refused write with invalid', async () => {
    const refused = new DkError({ error: 'Bad value.', kind: 'device', status: 'E_SET_VALUE' });
    const { point } = endpoint({
      get: async () => {
        throw refused;
      },
      put: async () => {
        throw refused;
      },
    });
    const channel = settingsOf(point);
    expect(await channel.getSetting('gain')).toEqual({
      ok: false,
      error: { code: 'invalid-stored', message: 'Bad value.' },
    });
    expect(await channel.putSetting('gain', Uint8Array.of(0, 0, 0, 1), 3)).toMatchObject({
      ok: false,
      error: { code: 'invalid' },
    });
  });
  it('carries the revision of a refused read into invalid-stored', async () => {
    const refused = Object.assign(new DkError({ error: 'Bad value.', kind: 'device', status: 'E_SET_VALUE' }), {
      revision: 9,
    });
    const channel = settingsOf(
      endpoint({
        get: async () => {
          throw refused;
        },
      }).point,
    );
    expect(await channel.getSetting('gain')).toEqual({
      ok: false,
      error: { code: 'invalid-stored', message: 'Bad value.', revision: 9 },
    });
  });
  it('maps each device status to its typed error', () => {
    const status = (name: string) => settingErrorOf(new DkError({ error: 'x', kind: 'device', status: name })).code;
    expect(status('E_MODULE_UI_CONFLICT')).toBe('conflict');
    expect(status('E_MODULE_UI_SCOPE')).toBe('overlay-scope');
    expect(status('E_AGAIN')).toBe('busy');
    expect(status('E_SET_VALUE')).toBe('invalid');
    expect(status('E_MSG')).toBe('invalid');
    expect(status('E_NOFILE')).toBe('unavailable');
    expect(settingErrorOf(new Error('The module page has closed.'))).toEqual({
      code: 'unavailable',
      message: 'The module page has closed.',
    });
  });
  it('returns a thrown exchange as a typed error and a corrupt stored value as invalid-stored', async () => {
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
    expect(await bad.get('gain')).toMatchObject({ ok: false, error: { code: 'invalid-stored' } });
  });
});

describe('createDeviceModuleChannel', () => {
  it('sends the progress of a file as events of its job and answers settings without an endpoint as unavailable', async () => {
    const sent = {
      ...files,
      send: async (_name: string, _bytes: Uint8Array, progress: (sent: number, total: number) => void) =>
        progress(1, 2),
    };
    const channel = createDeviceModuleChannel({
      settings: deviceSettingsChannel('Typed'),
      queries: deviceQueryChannel(),
      files: sent,
    });
    const events: unknown[] = [];
    channel.subscribe((e) => events.push(e));
    await channel.sendFile(4, 'a.dkbs', Uint8Array.of(1, 2));
    expect(events).toEqual([{ kind: 'file-progress', job: 4, done: 1, total: 2 }]);
    expect(await channel.getSetting('gain')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    expect(await channel.describeSettings()).toEqual({ name: 'Typed', version: 1, settings: [] });
  });

  it('passes saves to the host function after checking the names, and refuses without one', async () => {
    const got: string[][] = [];
    const parts = { settings: deviceSettingsChannel('Typed'), queries: deviceQueryChannel(), files };
    const channel = createDeviceModuleChannel({
      ...parts,
      saveFiles: async (list) => {
        got.push(list.map((f) => f.name));
        return { saved: list.length, cancelled: false, where: '/out' };
      },
    });
    expect(await channel.saveFiles([{ name: 'a.wav', bytes: Uint8Array.of(1) }])).toEqual({
      saved: 1,
      cancelled: false,
      where: '/out',
    });
    await expect(channel.saveFiles([{ name: '../a', bytes: Uint8Array.of(1) }])).rejects.toThrow(/plain file name/);
    expect(got).toEqual([['a.wav']]);
    await expect(createDeviceModuleChannel(parts).saveFiles([])).rejects.toThrow(/cannot save/);
  });
});

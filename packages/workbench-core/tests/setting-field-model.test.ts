import { createRoot } from 'solid-js';
import { describe, expect, it } from 'vitest';
import { createMemoryModuleChannel, createModuleClient, type SettingsSchema } from '@digi/module-ui';
import { createSettingFieldModel } from '../ui/module-ui/setting-field-model.ts';

const schema = {
  module: 'test/form',
  name: 'Form',
  version: 2,
  settings: [
    { id: 'name', label: 'Name', kind: 'text', apply: 'immediate', maxBytes: 8 },
    {
      id: 'gain',
      label: 'Gain',
      kind: 'integer',
      apply: 'immediate',
      min: 0,
      max: 100,
      step: 1,
      decimals: 2,
      unit: 'dB',
      default: 0.5,
    },
    { id: 'on', label: 'On', kind: 'boolean', apply: 'immediate', default: true },
    {
      id: 'mode',
      label: 'Mode',
      kind: 'choice',
      apply: 'immediate',
      choices: ['Off', 'Soft', 'Hard'],
      default: 'Soft',
    },
  ],
} as const satisfies SettingsSchema;

function form(
  id: (typeof schema.settings)[number]['id'],
  options: Parameters<typeof createMemoryModuleChannel>[1] = {},
) {
  const { channel } = createMemoryModuleChannel(schema, options);
  const settings = createModuleClient(channel).settings(schema as SettingsSchema);
  return createRoot(() =>
    createSettingFieldModel(
      schema.settings.find((s) => s.id === id)!,
      settings,
    ),
  );
}

describe('a generic form field', () => {
  it('shows the schema default for an unset integer and saves a value of its own', async () => {
    const model = form('gain');
    expect(model.canSave()).toBe(false);
    await model.read();
    expect(model.saved()).toEqual({ value: 0.5, unset: true });
    expect(model.draft()).toBe(0.5);
    expect(model.canSave()).toBe(false);
    model.edit(0.75);
    expect(model.canSave()).toBe(true);
    await model.save();
    expect(model.saved()).toEqual({ value: 0.75, unset: false });
    expect(model.canReset()).toBe(true);
    await model.reset();
    expect(model.saved()).toEqual({ value: 0.5, unset: true });
    expect(model.draft()).toBe(0.5);
  });
  it('lets an unset field store its default and an unset text store the empty string once edited', async () => {
    const on = form('on');
    await on.read();
    on.edit(true);
    expect(on.canSave()).toBe(true);
    const name = form('name');
    await name.read();
    expect(name.saved()).toEqual({ value: null, unset: true });
    name.edit('');
    await name.save();
    expect(name.saved()).toEqual({ value: '', unset: false });
  });
  it('reads and saves a choice by its label and a boolean as a flag', async () => {
    const mode = form('mode');
    await mode.read();
    expect(mode.draft()).toBe('Soft');
    mode.edit('Hard');
    await mode.save();
    expect(mode.saved()).toEqual({ value: 'Hard', unset: false });
    const on = form('on');
    await on.read();
    on.edit(false);
    await on.save();
    expect(on.saved()).toEqual({ value: false, unset: false });
  });
  it('keeps a value the module refuses as an error line and the saved state', async () => {
    const model = form('gain');
    await model.read();
    model.edit(2);
    await model.save();
    expect(model.error()).toMatch(/0\.00 to 1\.00/);
    expect(model.saved()).toEqual({ value: 0.5, unset: true });
    await model.read();
    expect(model.error()).toBe('');
  });
  it('shows the default and says so when the device refuses the stored value', async () => {
    const { channel } = createMemoryModuleChannel(schema);
    const refusing = {
      ...channel,
      getSetting: async () => ({ ok: false, error: { code: 'invalid-stored', message: 'bad' } }) as const,
    };
    const settings = createModuleClient(refusing).settings(schema as SettingsSchema);
    const model = createRoot(() => createSettingFieldModel(schema.settings[3], settings));
    await model.read();
    expect(model.storedRefused()).toBe(true);
    expect(model.error()).toBe('');
    expect(model.draft()).toBe('Soft');
    expect(model.saved()).toEqual({ value: 'Soft', unset: true });
  });
});

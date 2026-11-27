import { describe, expect, it } from 'vitest';
import type { DkModule } from '@digi/dk-session';
import { createModuleClient, MemorySettingsStore, settingError, type DescribedSchema } from '@digi/module-ui';
import { createDeviceModuleChannel } from '@digi/workbench-core/module-ui/device-module-channel.ts';
import { describeDeviceSettings, settingErrorOf } from '@digi/workbench-core/module-ui/device-settings.ts';
import { createPreviewSettings, previewRefusal } from '../preview/module-settings';

const schema: DescribedSchema = {
  name: 'Typed',
  version: 1,
  settings: [{ id: 'boot', label: 'Boot', kind: 'text', apply: 'immediate', maxBytes: 8 }],
};
const files = {
  list: async () => [],
  read: async () => new Uint8Array(),
  send: async () => {},
  remove: async () => {},
};

function previewPage() {
  const card = {
    files: new Map<string, Uint8Array>(),
    boot: { modules: [{ index: 1, uiInstance: 1, name: 'Typed' } as DkModule] },
    settings: new Map<string, MemorySettingsStore>(),
  };
  const endpoint = createPreviewSettings(
    new Map([['Typed', schema]]),
    async () => card,
    1,
    1,
    () => true,
  );
  const client = () => createModuleClient(createDeviceModuleChannel({ schema, endpoint, files }));
  return { endpoint, client };
}

// The preview throws what a device sends, so the real adapter maps it.
describe('the preview settings endpoint through the device adapter', () => {
  it('reports its schema without a descriptor', async () => {
    expect(await describeDeviceSettings(previewPage().endpoint)).toEqual(schema);
  });
  it('maps a stale revision to conflict', async () => {
    const { client } = previewPage();
    const a = client().settings(schema);
    const b = client().settings(schema);
    await a.get('boot');
    await b.get('boot');
    expect(await b.put('boot', 'b')).toMatchObject({ ok: true });
    expect(await a.put('boot', 'a')).toMatchObject({ ok: false, error: { code: 'conflict' } });
  });
  it('maps an overlay refusal to overlay-scope and an unknown failure to unavailable', () => {
    expect(settingErrorOf(previewRefusal(settingError('overlay-scope', 'On the device.'))).code).toBe('overlay-scope');
    expect(settingErrorOf(previewRefusal(settingError('busy', 'Full.'))).code).toBe('busy');
    expect(settingErrorOf(previewRefusal(settingError('unavailable', 'Gone.'))).code).toBe('unavailable');
  });
});

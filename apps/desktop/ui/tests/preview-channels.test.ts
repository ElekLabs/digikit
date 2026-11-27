import { describe, expect, it } from 'vitest';
import type { DkModule } from '@digi/dk-session';
import { createModuleClient, type ModuleFileClient } from '@digi/module-ui';
import { schema as sample } from '../../../../tests/fixtures/modules/settings-a/workbench/gen/settings';
import { schema as bootscreen } from '../../../../modules/tool/tools/bootscreen/workbench/gen/settings';
import { createPreviewChannels } from '../preview/module-channels';
import { previewSampleIds, previewSettingsSchemas } from '../preview/module-schemas';

const module = (name: string) => ({ name, index: 1, uiInstance: 1 }) as DkModule;
const noFiles = (held: { name: string; size: number }[] = []): ModuleFileClient => ({
  list: async () => held,
  read: async () => new Uint8Array(),
  send: async () => {},
  remove: async () => {},
});
const schemas = () => previewSettingsSchemas();

function open(factory: ReturnType<typeof createPreviewChannels>, target: string, live = { on: true }) {
  const channel = factory({ target, module: module(sample.module), current: () => live.on });
  return createModuleClient(channel).settings(sample);
}

describe('the preview channels of simulated targets', () => {
  it('reads each module id and name from its generated schema, and seeds the sample the fixtures ship', () => {
    expect(schemas().get(sample.module)).toMatchObject({ name: 'Boot settings' });
    expect(schemas().get(bootscreen.module)).toMatchObject({ name: 'Boot screen' });
    expect(previewSampleIds).toContain(sample.module);
  });
  it('keeps one settings store per target across reloads of the instance and shares none between targets', async () => {
    const factory = createPreviewChannels(schemas(), () => noFiles());
    const first = open(factory, 'midi:A');
    await first.get('boot');
    await first.put('boot', 'A');
    const reloaded = open(factory, 'midi:A');
    expect(await reloaded.get('boot')).toMatchObject({ ok: true, value: 'A' });
    expect(await open(factory, 'midi:B').get('boot')).toMatchObject({ ok: true, value: null, unset: true });
  });
  it('fails the calls of an instance that has unloaded or whose target went away, without touching the store of its target', async () => {
    const factory = createPreviewChannels(schemas(), () => noFiles());
    const live = { on: true };
    const old = open(factory, 'midi:A', live);
    await old.get('boot');
    live.on = false;
    expect(await old.put('boot', 'late')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    expect(await open(factory, 'midi:A').get('boot')).toMatchObject({ ok: true, value: null });
  });
  it('refuses a module the preview has no schema for', () => {
    const factory = createPreviewChannels(schemas(), () => noFiles());
    expect(() => factory({ target: 'midi:A', module: module('no/such'), current: () => true })).toThrow(/no settings/);
  });
  it('accepts a file-name setting only for a file of its extension that the card holds', async () => {
    const factory = createPreviewChannels(schemas(), () =>
      noFiles([
        { name: 'intro.dkbs', size: 4 },
        { name: 'empty.dkbs', size: 0 },
      ]),
    );
    const channel = factory({ target: 'midi:A', module: module(bootscreen.module), current: () => true });
    const settings = createModuleClient(channel).settings(bootscreen);
    await settings.get('active');
    for (const name of ['empty.dkbs', 'gone.dkbs', 'intro.txt'])
      expect(await settings.put('active', name)).toMatchObject({
        ok: false,
        error: { code: 'invalid', message: /missing/ },
      });
    expect(await settings.put('active', 'intro.dkbs')).toMatchObject({ ok: true, value: 'intro.dkbs' });
  });
});

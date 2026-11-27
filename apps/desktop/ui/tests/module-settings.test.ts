import { describe, expect, it } from 'vitest';
import { createSettingsSample, descriptor } from '../../../../tests/fixtures/modules/settings-a/workbench/sample';
import manifest from '../../../../tests/fixtures/modules/settings-a/module.toml?raw';
import { readModuleDescriptor } from '@digi/workbench-core/module-ui/descriptor.ts';

describe('the settings sample from workbench.toml', () => {
  it('reads the startup snapshot across edits and takes a fresh snapshot on reload', async () => {
    const target = createSettingsSample();
    const client = target.boot(true)!(() => true);
    expect(await client.readAtInit()).toBeUndefined();
    await client.settings.put('boot', 'first', 0);
    expect(await client.readAtInit()).toBeUndefined();
    const next = target.boot(true)!(() => true);
    expect(await next.readAtInit()).toBe('first');
    await expect(client.settings.put('boot', 'late', 1)).rejects.toThrow(/closed/);
    target.boot(false);
    await expect(next.readAtInit()).rejects.toThrow(/closed/);
  });
  it('validates UTF-8 byte lengths and revisions and keeps empty distinct from unset', async () => {
    const client = createSettingsSample().boot(true)!(() => true);
    const { settings } = client;
    expect(descriptor.settings[0]?.maxBytes).toBe(16);
    const saved = await settings.put('boot', '🍋'.repeat(4), 0);
    await expect(settings.put('boot', '🍋'.repeat(5), saved.revision)).rejects.toThrow(/16 bytes/);
    await expect(settings.reset('boot', 0)).rejects.toThrow(/changed/);
    const empty = await settings.put('boot', '', saved.revision);
    expect(empty.value).toBe('');
    expect((await settings.reset('boot', empty.revision)).value).toBeUndefined();
    await expect(settings.get('private')).rejects.toThrow(/Unknown/);
  });
  it('reopening on another target never shares the first target’s values', async () => {
    let current = true;
    const a = createSettingsSample().boot(true)!(() => current);
    const b = createSettingsSample().boot(true)!(() => true);
    await a.settings.put('boot', 'A', 0);
    current = false;
    await expect(a.settings.reset('boot', 1)).rejects.toThrow(/closed/);
    expect((await b.settings.get('boot')).value).toBeUndefined();
  });
  it('can describe a standard form with no companion and rejects unsupported formats', () => {
    const form = '[workbench]\nversion=1\n[[setting]]\nid="name"\nlabel="Name"\ntype="text"\nmax_bytes=8';
    expect(readModuleDescriptor(form, '[module]\nid="test/example"\nname="Example module"').name).toBe(
      'Example module',
    );
    expect(readModuleDescriptor(form, manifest).component).toBeUndefined();
    expect(() => readModuleDescriptor(form.replace('version=1', 'version=2'), manifest)).toThrow(/version/);
  });
});

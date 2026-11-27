import { describe, expect, it } from 'vitest';
import { createSettingsSample, descriptor } from '../../../../tests/fixtures/modules/settings-a/workbench/sample';

describe('the settings sample from its generated schema', () => {
  it('reads the startup snapshot across edits and takes a fresh snapshot on reload', async () => {
    const target = createSettingsSample();
    const client = target.boot(true)!(() => true);
    expect(await client.readAtInit()).toBeUndefined();
    await client.settings.get('boot');
    await client.settings.put('boot', 'first');
    expect(await client.readAtInit()).toBeUndefined();
    const next = target.boot(true)!(() => true);
    expect(await next.readAtInit()).toBe('first');
    expect(await client.settings.put('boot', 'late')).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    target.boot(false);
    await expect(next.readAtInit()).rejects.toThrow(/closed/);
  });
  it('validates UTF-8 byte lengths and revisions and keeps empty distinct from unset', async () => {
    const client = createSettingsSample().boot(true)!(() => true);
    const { settings } = client;
    expect(descriptor.schema.settings[0]).toMatchObject({ kind: 'text', maxBytes: 16 });
    expect(descriptor.name).toBe('Boot settings');
    const first = await settings.get('boot');
    expect(first).toMatchObject({ ok: true, value: null, unset: true });
    const saved = await settings.put('boot', '🍋'.repeat(4));
    expect(saved).toMatchObject({ ok: true, revision: 1 });
    expect(await settings.put('boot', '🍋'.repeat(5))).toMatchObject({
      ok: false,
      error: { code: 'invalid', message: /16 bytes/ },
    });
    expect(await settings.reset('boot', 0)).toMatchObject({ ok: false, error: { code: 'conflict' } });
    expect(await settings.put('boot', '')).toMatchObject({ ok: true, value: '', unset: false });
    expect(await settings.reset('boot')).toMatchObject({ ok: true, value: null, unset: true });
  });
  it('reopening on another target never shares the first target’s values', async () => {
    let current = true;
    const a = createSettingsSample().boot(true)!(() => current);
    const b = createSettingsSample().boot(true)!(() => true);
    await a.settings.get('boot');
    await a.settings.put('boot', 'A');
    current = false;
    expect(await a.settings.reset('boot')).toMatchObject({
      ok: false,
      error: { code: 'unavailable', message: /closed/ },
    });
    expect(await b.settings.get('boot')).toMatchObject({ ok: true, value: null });
  });
});

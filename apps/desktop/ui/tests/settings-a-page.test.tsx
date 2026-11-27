// @vitest-environment happy-dom
import { render } from 'solid-js/web';
import { describe, expect, it, vi } from 'vitest';
import { createMemoryModuleChannel, createModuleClient } from '@digi/module-ui';
import page from '../../../../tests/fixtures/modules/settings-a/workbench/index';
import { schema } from '../../../../tests/fixtures/modules/settings-a/workbench/gen/settings';

describe('the settings-a fixture page', () => {
  it('shows the boot value a client stored, read through the module channel', async () => {
    const { channel } = createMemoryModuleChannel(schema);
    const client = createModuleClient(channel);
    const settings = client.settings(schema);
    await settings.get('boot');
    expect(await settings.put('boot', 'intro.dkbs')).toMatchObject({ ok: true });
    const host = document.createElement('div');
    const dispose = render(() => <page.Page client={client} />, host);
    await vi.waitFor(() => expect(host.textContent).toContain('Boot value now: intro.dkbs'));
    expect(host.textContent).toContain('take effect the next time the module starts');
    dispose();
  });
});

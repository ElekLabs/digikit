// @vitest-environment happy-dom
import { render } from 'solid-js/web';
import { describe, expect, it, vi } from 'vitest';
import { createMemoryModuleChannel, createModuleClient } from '@digi/module-ui';
import previewQueries from '../../../../tests/fixtures/modules/settings-a/workbench/preview';
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

  it('shows not set while the boot value is unset', async () => {
    const { channel } = createMemoryModuleChannel(schema);
    const host = document.createElement('div');
    const dispose = render(() => <page.Page client={createModuleClient(channel)} />, host);
    await vi.waitFor(() => expect(host.textContent).toContain('Boot value now: not set'));
    dispose();
  });

  it('shows the boot value at init from the query, and an error state when the module cannot answer', async () => {
    const open = (service?: ReturnType<typeof previewQueries>) => {
      const host = document.createElement('div');
      const { channel } = createMemoryModuleChannel(schema, { service });
      return { host, dispose: render(() => <page.Page client={createModuleClient(channel)} />, host) };
    };
    const answered = open(previewQueries((id) => (id === 'boot' ? new TextEncoder().encode('intro.dkbs') : undefined)));
    await vi.waitFor(() => expect(answered.host.textContent).toContain('Boot value at init: intro.dkbs'));
    answered.dispose();
    const unset = open(previewQueries(() => undefined));
    await vi.waitFor(() => expect(unset.host.textContent).toContain('Boot value at init: not set'));
    unset.dispose();
    const none = open();
    await vi.waitFor(() => expect(none.host.textContent).toContain('could not answer (unknown-method)'));
    none.dispose();
  });
});

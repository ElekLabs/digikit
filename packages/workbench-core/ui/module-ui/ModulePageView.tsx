import { For, Show, createSignal, onCleanup } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import { formFields, modulePageTabs, type ModulePageTab } from '../../src/module-ui/module-page-tabs';
import type { ModulePage } from '../../src/module-ui/module-page';
import SettingField from './SettingField';
import './module-page.css';

const TAB_LABEL: Record<ModulePageTab, string> = { page: 'Page', settings: 'Settings' };

export default function ModulePageView(props: { page: ModulePage; current: () => boolean }) {
  let mounted = true;
  onCleanup(() => {
    mounted = false;
  });
  const client = props.page.open(() => mounted && props.current());
  const tabs = modulePageTabs(props.page.descriptor);
  const [tab, setTab] = createSignal<ModulePageTab | undefined>(tabs[0]);
  const fields = () => formFields(props.page.descriptor.schema);
  return (
    <main class="module-page">
      <h2>{props.page.descriptor.name}</h2>
      <Show when={client.Panel}>{(panel) => <Dynamic component={panel()} />}</Show>
      <Show when={tabs.length > 1}>
        <div class="module-tabs" role="tablist" aria-label="Module">
          <For each={tabs}>
            {(t) => (
              <button
                type="button"
                role="tab"
                class="module-tab"
                classList={{ on: tab() === t }}
                aria-selected={tab() === t}
                onClick={() => setTab(t)}
              >
                {TAB_LABEL[t]}
              </button>
            )}
          </For>
        </div>
      </Show>
      <Show when={tab() === 'page' && client.Page}>{(page) => <Dynamic component={page()} />}</Show>
      <Show when={tab() === 'settings' && client.settings}>
        {(settings) => (
          <section class="module-settings" aria-label="Settings">
            <For each={fields()}>{(field) => <SettingField field={field} settings={settings()} />}</For>
          </section>
        )}
      </Show>
    </main>
  );
}

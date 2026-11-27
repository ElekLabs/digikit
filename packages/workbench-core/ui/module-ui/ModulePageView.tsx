import { For, Show, onCleanup } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import type { Setting } from '@digi/module-ui';
import type { ModulePage } from '../../src/module-ui/module-page';
import SettingField from './SettingField';
import './module-page.css';

type FormSetting = Exclude<Setting, { kind: 'file-name' }>;

export default function ModulePageView(props: { page: ModulePage; current: () => boolean }) {
  let mounted = true;
  onCleanup(() => {
    mounted = false;
  });
  const client = props.page.open(() => mounted && props.current());
  // A file-name setting has its own control in the module's file panel.
  const fields = () =>
    props.page.descriptor.schema.settings.filter((setting): setting is FormSetting => setting.kind !== 'file-name');
  return (
    <main class="module-page">
      <h2>{props.page.descriptor.name}</h2>
      <Show when={fields().length > 0 && client.settings}>
        {(settings) => (
          <section class="module-settings" aria-label="Settings">
            <For each={fields()}>{(field) => <SettingField field={field} settings={settings()} />}</For>
          </section>
        )}
      </Show>
      <Show when={client.Panel}>{(panel) => <Dynamic component={panel()} />}</Show>
    </main>
  );
}

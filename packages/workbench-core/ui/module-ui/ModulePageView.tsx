import { For, Show, createSignal, onCleanup, onMount } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import type { ModulePage, SettingsClient, SettingValue } from '../../src/module-ui/module-page';
import { checkText, type TextSetting } from '../../src/module-ui/descriptor';
import './module-page.css';

function TextField(props: { field: TextSetting; settings: SettingsClient }) {
  const [saved, setSaved] = createSignal<SettingValue>();
  const [draft, setDraft] = createSignal('');
  const [edited, setEdited] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  async function update(action: 'read' | 'save' | 'reset') {
    setBusy(true);
    setError('');
    try {
      const key = props.field.id;
      if (action === 'save') checkText(props.field, draft());
      const next =
        action === 'read'
          ? await props.settings.get(key)
          : action === 'save'
            ? await props.settings.put(key, draft(), saved()!.revision)
            : await props.settings.reset(key, saved()!.revision);
      setSaved(next);
      setDraft(next.value ?? '');
      setEdited(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }
  onMount(() => void update('read'));
  return (
    <div class="module-field">
      <label>
        <span>{props.field.label}</span>
        <small>Up to {props.field.maxBytes} bytes</small>
        <input
          type="text"
          value={draft()}
          placeholder={saved()?.value === undefined ? 'Unset' : undefined}
          disabled={!saved() || busy()}
          onInput={(e) => {
            setEdited(true);
            setDraft(e.currentTarget.value);
          }}
        />
      </label>
      <button
        class="btn primary"
        disabled={busy() || !edited() || !saved() || draft() === saved()?.value}
        onClick={() => void update('save')}
      >
        Save
      </button>
      <button class="btn" disabled={busy() || saved()?.value === undefined} onClick={() => void update('reset')}>
        Reset
      </button>
      <Show when={error()}>
        <span role="alert">{error()}</span>
        <button class="btn" disabled={busy()} onClick={() => void update('read')}>
          Read again
        </button>
      </Show>
    </div>
  );
}

export default function ModulePageView(props: { page: ModulePage; current: () => boolean }) {
  let mounted = true;
  onCleanup(() => {
    mounted = false;
  });
  const client = props.page.open(() => mounted && props.current());
  return (
    <main class="module-page">
      <h2>{props.page.descriptor.name}</h2>
      <Show when={props.page.descriptor.settings.length > 0 && client.settings}>
        {(settings) => (
          <section class="module-settings" aria-label="Settings">
            <For each={props.page.descriptor.settings}>
              {(field) => <TextField field={field} settings={settings()} />}
            </For>
          </section>
        )}
      </Show>
      <Show when={client.Panel}>{(panel) => <Dynamic component={panel()} />}</Show>
    </main>
  );
}

import { For, Show, createSignal, onCleanup, onMount } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import type { DynamicSettings, TextSetting } from '@digi/module-ui';
import type { ModulePage } from '../../src/module-ui/module-page';
import './module-page.css';

type Saved = { value: string | null; unset: boolean };

function TextField(props: { field: TextSetting; settings: DynamicSettings }) {
  const [saved, setSaved] = createSignal<Saved>();
  const [draft, setDraft] = createSignal('');
  const [edited, setEdited] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  async function update(action: 'read' | 'save' | 'reset') {
    setBusy(true);
    setError('');
    try {
      const key = props.field.id;
      const next =
        action === 'read'
          ? await props.settings.get(key)
          : action === 'save'
            ? await props.settings.put(key, draft())
            : await props.settings.reset(key);
      if (!next.ok) {
        setError(next.error.message);
        return;
      }
      const value = next.value as string | null;
      setSaved({ value, unset: next.unset });
      setDraft(value ?? '');
      setEdited(false);
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
          placeholder={saved()?.unset ? 'Unset' : undefined}
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
      <button class="btn" disabled={busy() || !saved() || saved()!.unset} onClick={() => void update('reset')}>
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
  const fields = () =>
    props.page.descriptor.schema.settings.filter((setting): setting is TextSetting => setting.kind === 'text');
  return (
    <main class="module-page">
      <h2>{props.page.descriptor.name}</h2>
      <Show when={fields().length > 0 && client.settings}>
        {(settings) => (
          <section class="module-settings" aria-label="Settings">
            <For each={fields()}>{(field) => <TextField field={field} settings={settings()} />}</For>
          </section>
        )}
      </Show>
      <Show when={client.Panel}>{(panel) => <Dynamic component={panel()} />}</Show>
    </main>
  );
}

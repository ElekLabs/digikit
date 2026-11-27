import { For, Show, createEffect, createMemo, createResource, createSignal } from 'solid-js';
import type { PublicField } from '@digi/dk-session';
import type { SettingsClient } from '../../src/module-ui/module-page';
import { moduleFilePath } from '../../src/modules/dktransfer';
import { errorLine } from '../../src/modules/backend';
import type { PageCtx } from '../transfer/TransferParts';

export default function FileChoice(props: {
  ctx: PageCtx;
  id: string;
  field: Extract<PublicField, { kind: 'file-name' }>;
  settings: SettingsClient;
  files: () => { name: string; size: number }[];
  refresh: () => unknown;
}) {
  const [saved, { mutate, refetch }] = createResource(
    () => [props.field.id, props.refresh()] as const,
    ([key]) => props.settings.get(key),
  );
  const [choice, setChoice] = createSignal('');
  const [notice, setNotice] = createSignal('');
  createEffect(() => {
    if (!saved.loading && !saved.error) setChoice(saved()?.value ?? '');
  });
  const choices = createMemo(() =>
    props
      .files()
      .filter(({ name, size }) => {
        if (
          !size ||
          !name.toLowerCase().endsWith(`.${props.field.extension}`) ||
          name.length <= props.field.extension.length + 1 ||
          name.length > props.field.maxBytes ||
          !/^[!-~ ]+$/.test(name) ||
          name.includes('\\')
        )
          return false;
        try {
          moduleFilePath(props.id, name);
          return true;
        } catch {
          return false;
        }
      })
      .map(({ name }) => name),
  );
  const snapshot = () => (saved.error ? undefined : saved.latest);
  const missing = () => {
    const value = snapshot()?.value;
    return value && !choices().includes(value) ? value : undefined;
  };
  const dirty = () => choice() !== (snapshot()?.value ?? '');
  const save = () =>
    props.ctx.run('Saving', async () => {
      const old = snapshot();
      if (!old) return;
      setNotice('');
      const selected = choice();
      const next = selected
        ? await props.settings.put(props.field.id, selected, old.revision)
        : await props.settings.reset(props.field.id, old.revision);
      mutate(next);
      setNotice(props.field.apply === 'next-restart' ? 'Saved. Applies on next restart.' : 'Saved.');
    });

  return (
    <div class="module-file-choice">
      <label>
        <span>{props.field.label}</span>
        <select
          value={choice()}
          disabled={!!props.ctx.busy() || saved.loading || !!saved.error || !snapshot()}
          onChange={(event) => {
            setChoice(event.currentTarget.value);
            setNotice('');
          }}
        >
          <option value="" selected={choice() === ''}>
            {props.field.resetLabel}
          </option>
          <Show when={missing()}>
            {(name) => (
              <option value={name()} selected={choice() === name()} disabled>
                {name()} (missing)
              </option>
            )}
          </Show>
          <For each={choices()}>
            {(name) => (
              <option value={name} selected={choice() === name}>
                {name}
              </option>
            )}
          </For>
        </select>
      </label>
      <button
        type="button"
        class="act primary"
        disabled={!!props.ctx.busy() || saved.loading || !!saved.error || !dirty()}
        onClick={() => void save()}
      >
        Save
      </button>
      <Show when={saved.error}>
        <p class="err" role="alert">
          {errorLine(saved.error)}
        </p>
        <button type="button" class="act" onClick={() => void refetch()}>
          Read again
        </button>
      </Show>
      <Show
        when={notice()}
        fallback={
          <Show when={props.field.apply === 'next-restart'}>
            <p class="meta">Applies on next restart.</p>
          </Show>
        }
      >
        <p class="meta" role="status">
          {notice()}
        </p>
      </Show>
    </div>
  );
}

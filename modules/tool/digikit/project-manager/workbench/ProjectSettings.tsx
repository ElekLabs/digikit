import { For, Match, Show, Switch, createSignal, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { ProjectManager, type GetSettingsReply } from './gen/project_manager_pb.js';
import { queryErrorText, valueView } from './project-text';

/** How long to wait before asking again for a project the device is still reading. */
export const SETTINGS_RETRY_MS = 200;

type Entries = GetSettingsReply['entries'];
type State = 'loading' | 'ready' | 'missing' | 'error';

/** The rows of one module's saved settings in a project, to sit under the module's row. The first page is asked for when the rows are created; "More" asks for
 * the reply's `next`. While the device reads the project's file the reply says `loading` and the rows ask again every SETTINGS_RETRY_MS; they stop when they are
 * removed (collapse, other selection, page closed). A project that changed since `version` calls `onChanged`. */
export default function ProjectSettings(props: {
  client: ModuleClient;
  slot: number;
  module: string;
  version: number;
  onChanged: () => void;
}) {
  // oxlint-disable-next-line solid/reactivity -- the rows are made for one project, module and version and are removed when one changes
  const { client, slot, module, version, onChanged } = props;
  const service = client.service(ProjectManager);
  const [entries, setEntries] = createSignal<Entries>([]);
  const [next, setNext] = createSignal(0);
  const [state, setState] = createSignal<State>('loading');
  const [error, setError] = createSignal('');
  let active = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => {
    active = false;
    clearTimeout(timer);
  });

  const ask = async (cursor: number) => {
    setState('loading');
    const result = await service.getSettings({ slot, module, version, cursor });
    if (!active) return;
    if (!result.ok) {
      setError(queryErrorText(result.error));
      return setState('error');
    }
    const reply = result.value;
    if (reply.loading) {
      timer = setTimeout(() => void ask(cursor), SETTINGS_RETRY_MS);
      return;
    }
    if (reply.changed) return onChanged();
    if (!reply.found) return setState('missing');
    setEntries((have) => [...have, ...reply.entries]);
    setNext(reply.next);
    setState('ready');
  };
  void ask(0);

  return (
    <>
      <For each={entries()}>
        {(entry) => {
          const view = valueView(entry.value, entry.valueLength);
          return (
            <li class="pm-setting">
              <span class="name">
                <span class="nm">{entry.key}</span>
              </span>
              <span class="pm-value" classList={{ hex: view.hex }}>
                {view.text}
                <Show when={view.truncated}>…</Show>
              </span>
              <Show when={view.truncated}>
                <span class="size">{entry.valueLength} bytes</span>
              </Show>
            </li>
          );
        }}
      </For>
      <Switch>
        <Match when={state() === 'loading'}>
          <li class="pm-setting" role="status">
            <span class="empty">Loading…</span>
          </li>
        </Match>
        <Match when={state() === 'missing'}>
          <li class="pm-setting">
            <span class="empty">The project holds no settings for this module.</span>
          </li>
        </Match>
        <Match when={state() === 'error'}>
          <li class="pm-setting" role="alert">
            <span class="pm-error">{error()}</span>
            <button type="button" class="mini" onClick={() => void ask(next())}>
              Try again
            </button>
          </li>
        </Match>
        <Match when={next() > 0}>
          <li class="pm-setting">
            <button type="button" class="act small pm-more" onClick={() => void ask(next())}>
              More
            </button>
          </li>
        </Match>
      </Switch>
    </>
  );
}

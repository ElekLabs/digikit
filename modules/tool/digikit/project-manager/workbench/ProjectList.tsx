import { For, Show, createSignal, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { ProjectManager, type ListReply } from './gen/project_manager_pb.js';
import { projectTitle, queryErrorText } from './project-text';

type Items = ListReply['items'];

/** The projects of the device by number and name, a page of 16 at a time (`next` asks for the following page). */
export default function ProjectList(props: {
  client: ModuleClient;
  selected: number | undefined;
  onSelect: (slot: number) => void;
}) {
  // oxlint-disable-next-line solid/reactivity -- the client belongs to the page and does not change while it is open
  const service = props.client.service(ProjectManager);
  const [items, setItems] = createSignal<Items>([]);
  const [next, setNext] = createSignal(0);
  const [error, setError] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  let open = true;
  onCleanup(() => {
    open = false;
  });

  async function load(first: number) {
    setBusy(true);
    const result = await service.list({ firstSlot: first });
    if (!open) return;
    setBusy(false);
    if (!result.ok) {
      setError(queryErrorText(result.error));
      return;
    }
    setError('');
    setItems((have) => [...have, ...result.value.items]);
    setNext(result.value.next);
  }
  void load(0);

  return (
    <section class="pm-list" aria-label="Projects">
      <h3>Projects</h3>
      <Show when={items().length}>
        <ul class="rows">
          <For each={items()}>
            {(item) => (
              <li classList={{ sel: props.selected === item.slot }} onClick={() => props.onSelect(item.slot)}>
                <span class="name">
                  <button type="button" class="nm pm-project" aria-pressed={props.selected === item.slot}>
                    {projectTitle(item.slot, item.name)}
                  </button>
                  <Show when={item.current}>
                    <span class="rtag ok">current</span>
                  </Show>
                  <Show
                    when={item.dataUnknown}
                    fallback={
                      <Show when={!item.hasData}>
                        <span class="rtag">no digikit data</span>
                      </Show>
                    }
                  >
                    <span class="rtag">…</span>
                  </Show>
                </span>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={error()}>
        <p class="pm-error" role="alert">
          {error()}{' '}
          <button type="button" class="mini" onClick={() => void load(next())} disabled={busy()}>
            Try again
          </button>
        </p>
      </Show>
      <Show when={!error() && next() > 0}>
        <button type="button" class="act small pm-more" onClick={() => void load(next())} disabled={busy()}>
          More projects
        </button>
      </Show>
    </section>
  );
}

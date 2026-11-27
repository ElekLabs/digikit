import { For, Match, Show, Switch, createEffect, createSignal, on, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { ProjectManager, type GetReply } from './gen/project_manager_pb.js';
import { hex32, projectTitle, queryErrorText, slotNumber } from './project-text';

/** How long to wait before asking again for a project the device is still reading. */
export const LOADING_RETRY_MS = 200;

type Shown = { state: 'loading' } | { state: 'error'; text: string } | { state: 'missing' } | { state: 'found'; reply: GetReply };

/** The details of the selected project. While the device reads a project's file the reply says `loading`: the page asks again every
 * LOADING_RETRY_MS until it does not, and stops when the selection changes or the page closes. */
export default function ProjectDetails(props: { client: ModuleClient; slot: number }) {
  // oxlint-disable-next-line solid/reactivity -- the client belongs to the page and does not change while it is open
  const service = props.client.service(ProjectManager);
  const [shown, setShown] = createSignal<Shown>({ state: 'loading' });

  createEffect(
    on(
      () => props.slot,
      (slot) => {
        let active = true;
        let timer: ReturnType<typeof setTimeout> | undefined;
        onCleanup(() => {
          active = false;
          clearTimeout(timer);
        });
        setShown({ state: 'loading' });
        const ask = async () => {
          const result = await service.get({ slot });
          if (!active) return;
          if (!result.ok) return setShown({ state: 'error', text: queryErrorText(result.error) });
          const reply = result.value;
          if (reply.loading) {
            setShown({ state: 'loading' });
            timer = setTimeout(() => void ask(), LOADING_RETRY_MS);
          } else setShown(reply.found ? { state: 'found', reply } : { state: 'missing' });
        };
        void ask();
      },
    ),
  );

  const found = () => {
    const now = shown();
    return now.state === 'found' ? now.reply : undefined;
  };
  const failure = () => {
    const now = shown();
    return now.state === 'error' ? now.text : undefined;
  };

  return (
    <section class="pm-details" aria-label="Project details">
      <Switch>
        <Match when={shown().state === 'loading'}>
          <p>Loading…</p>
        </Match>
        <Match when={failure()}>
          {(text) => (
            <p class="pm-error" role="alert">
              {text()}
            </p>
          )}
        </Match>
        <Match when={shown().state === 'missing'}>
          <p>The device has no data for project {slotNumber(props.slot)}.</p>
        </Match>
        <Match when={found()}>
          {(reply) => (
            <>
              <h3>{projectTitle(reply().slot, reply().name)}</h3>
              <dl>
                <dt>Number</dt>
                <dd>{slotNumber(reply().slot)}</dd>
                <dt>Name</dt>
                <dd>{reply().name || 'no name'}</dd>
                <dt>Write counter</dt>
                <dd>{reply().counter}</dd>
                <dt>Save stamp</dt>
                <dd>{hex32(reply().stamp)}</dd>
              </dl>
              <h4>Modules</h4>
              <Show when={reply().modules.length} fallback={<p>No modules.</p>}>
                <ul class="pm-modules">
                  <For each={reply().modules}>
                    {(module) => (
                      <li>
                        <span class="pm-title">{module.id}</span>
                        <span class="pm-note">{module.loaded ? 'loaded' : 'not loaded'}</span>
                        <span class="pm-note">
                          {module.settings} {module.settings === 1 ? 'setting' : 'settings'}
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </>
          )}
        </Match>
      </Switch>
    </section>
  );
}

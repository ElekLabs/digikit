import { For, Match, Show, Switch, createEffect, createSignal, on, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { ProjectManager, type GetReply } from './gen/project_manager_pb.js';
import ProjectSettings from './ProjectSettings';
import { hex32, queryErrorText, slotNumber } from './project-text';

/** How long to wait before asking again for a project the device is still reading. */
export const LOADING_RETRY_MS = 200;

type Shown = { state: 'loading' } | { state: 'error'; text: string } | { state: 'missing' } | { state: 'found'; reply: GetReply };

/** The details of the selected project. While the device reads a project's file the reply says `loading`: the page asks again every
 * LOADING_RETRY_MS until it does not, and stops when the selection changes or the page closes. */
export default function ProjectDetails(props: { client: ModuleClient; slot: number }) {
  // oxlint-disable-next-line solid/reactivity -- the client belongs to the page and does not change while it is open
  const service = props.client.service(ProjectManager);
  const [shown, setShown] = createSignal<Shown>({ state: 'loading' });
  // the module whose saved settings are open, and how many times the project was read again because it changed
  const [open, setOpen] = createSignal<string>();
  const [reads, setReads] = createSignal(0);

  createEffect(
    on(
      [() => props.slot, reads],
      ([slot]) => {
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
  // another project: nothing is open
  createEffect(
    on(
      () => props.slot,
      () => setOpen(undefined),
      { defer: true },
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
          <div class="pm-skeleton" role="status">
            <span class="pm-sr">Loading…</span>
            <span class="pm-bar big" />
            <span class="pm-bar" />
            <span class="pm-bar" />
            <span class="pm-bar short" />
          </div>
        </Match>
        <Match when={failure()}>
          {(text) => (
            <p class="pm-error" role="alert">
              {text()}
            </p>
          )}
        </Match>
        <Match when={shown().state === 'missing'}>
          <p class="empty">The device has no data for project {slotNumber(props.slot)}.</p>
        </Match>
        <Match when={found()}>
          {(reply) => (
            <>
              <h2 class="pm-heading">
                <span class="pm-number">{slotNumber(reply().slot)}</span>
                <span>{reply().name || 'no name'}</span>
              </h2>
              <dl class="pm-facts">
                <dt>Slot</dt>
                <dd>{slotNumber(reply().slot)}</dd>
                <dt>Write counter</dt>
                <dd>{reply().counter}</dd>
                <dt>Save stamp</dt>
                <dd>{hex32(reply().stamp)}</dd>
              </dl>
              <h4>Modules</h4>
              <Show when={reply().modules.length} fallback={<p class="empty">No modules.</p>}>
                <ul class="rows">
                  <For each={reply().modules}>
                    {(module) => (
                      <>
                      <li class="pm-module" classList={{ sel: open() === module.id }} onClick={() => setOpen((now) => (now === module.id ? undefined : module.id))}>
                        <span class="twisty" aria-hidden="true">
                          {open() === module.id ? '▾' : '▸'}
                        </span>
                        <span class="name">
                          <button type="button" class="nm pm-project" aria-expanded={open() === module.id}>
                            {module.id}
                          </button>
                          <span class="rtag" classList={{ ok: module.loaded }}>
                            {module.loaded ? 'loaded' : 'not loaded'}
                          </span>
                        </span>
                        <span class="size">
                          {module.settings} {module.settings === 1 ? 'setting' : 'settings'}
                        </span>
                      </li>
                      <Show when={open() === module.id}>
                        <Show
                          when={module.settings > 0}
                          fallback={
                            <li class="pm-setting">
                              <span class="empty">No settings saved.</span>
                            </li>
                          }
                        >
                          <ProjectSettings
                            client={props.client}
                            slot={reply().slot}
                            module={module.id}
                            version={reply().counter}
                            onChanged={() => setReads((n) => n + 1)}
                          />
                        </Show>
                      </Show>
                      </>
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

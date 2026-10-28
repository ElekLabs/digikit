// The read-only source of a library module, in a panel at the right of the Transfer page: the module's files (the site ships module.toml,
// README.md and src/**) and the chosen one as text. No editor. Opened by the Source link of a library row (ModulesMode.tsx); Esc or Close shuts it.
import { For, Show, createEffect, createResource, createSignal, on, onCleanup, onMount, type JSX } from 'solid-js';
import { errorLine } from '../modules/backend';
import { transferEnv } from './env';
import { setSourceId, sourceId } from './transfer-state';

export default function SourcePanel(): JSX.Element {
  return <Show when={sourceId()}>{(id) => <Panel id={id()} />}</Show>;
}

function Panel(props: { id: string }): JSX.Element {
  const files = () => transferEnv().source?.files(props.id) ?? [];
  // module.toml first, then the README, then the code: the order the site ships
  const [path, setPath] = createSignal(files().find((f) => f.startsWith('src/')) ?? files()[0] ?? '');
  createEffect(
    on(
      () => props.id,
      () => setPath(files().find((f) => f.startsWith('src/')) ?? files()[0] ?? ''),
      { defer: true },
    ),
  );
  const [text] = createResource(
    () => ({ id: props.id, path: path() }),
    async (k) => {
      if (!k.path) return '';
      try {
        return await transferEnv().source!.read(k.id, k.path);
      } catch (e) {
        return `Could not load ${k.path}: ${errorLine(e)}`;
      }
    },
  );
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') setSourceId(undefined);
  };
  onMount(() => window.addEventListener('keydown', onKey));
  onCleanup(() => window.removeEventListener('keydown', onKey));
  return (
    <aside class="source" aria-label={`Source of ${props.id}`}>
      <header class="sourcehead">
        <h3>Source</h3>
        <span class="meta">{props.id}</span>
        <span class="sp" />
        <button type="button" class="act small" onClick={() => setSourceId(undefined)}>
          Close
        </button>
      </header>
      <ul class="sourcefiles" role="listbox" aria-label="Files">
        <For each={files()}>
          {(f) => (
            <li>
              <button
                type="button"
                role="option"
                aria-selected={f === path()}
                classList={{ on: f === path() }}
                onClick={() => setPath(f)}
              >
                {f}
              </button>
            </li>
          )}
        </For>
      </ul>
      <pre class="sourcetext" tabindex={0} aria-label={path()}>
        {text.loading ? 'Loading…' : text()}
      </pre>
    </aside>
  );
}

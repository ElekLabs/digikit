import { For, Show, createMemo, createResource, createSignal } from 'solid-js';
import { deviceNameOf } from '@digi/workbench-core/data/device-profiles.ts';
import type { SymbolsHost } from '@digi/workbench-core/host/host.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import './firmware-symbols.css';

type SourceKind = Parameters<SymbolsHost['choose']>[0];

const kinds: { kind: SourceKind; label: string }[] = [
  { kind: 'cf', label: 'ColdFire database' },
  { kind: 'sharc', label: 'SHARC database' },
  { kind: 'map', label: 'Symbol map' },
];

const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error));

export default function FirmwareSymbols(props: {
  shown: boolean;
  symbols: SymbolsHost;
  source?: TargetBinding;
  imageRevision?: unknown;
  imageFor(source: TargetBinding): Promise<string | undefined>;
  targetName: string;
}) {
  const [working, setWorking] = createSignal(false);
  const [actionError, setActionError] = createSignal('');
  const [refreshCount, setRefreshCount] = createSignal(0);
  const [sources, { refetch }] = createResource(
    () => props.shown,
    async () => {
      try {
        return { files: await props.symbols.list(), error: '' };
      } catch (error) {
        return { files: [], error: errorMessage(error) };
      }
    },
  );
  const imageRequest = createMemo(() => {
    const source = props.source;
    if (!props.shown || !source || source.target.kind === 'none') return;
    return { source, revision: props.imageRevision, refresh: refreshCount() };
  });
  const [image] = createResource(imageRequest, async (request) => {
    try {
      return { request, sha256: (await props.imageFor(request.source))?.toLowerCase() };
    } catch {
      return { request };
    }
  });
  const activeHash = () => {
    const current = image.latest;
    return current?.request === imageRequest() ? current?.sha256 : undefined;
  };
  const busy = () => working() || sources.loading;
  const matched = () =>
    sources.latest?.files.some(
      (source) => !source.error && source.images.some((entry) => entry.sha256.toLowerCase() === activeHash()),
    );
  const imageStatus = () => {
    if (imageRequest() && image.loading) return 'Identifying firmware…';
    if (!activeHash()) return 'Firmware image unidentified';
    if (sources.loading && !sources.latest) return 'Reading sources…';
    if (sources.latest?.error) return 'Firmware identified';
    return matched() ? 'Matching symbols available' : 'No matching symbols';
  };

  async function run(action: () => Promise<void>) {
    if (busy()) return;
    setWorking(true);
    setActionError('');
    try {
      await action();
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      if (props.shown) await refetch();
      setWorking(false);
    }
  }

  async function refresh() {
    setRefreshCount((count) => count + 1);
    await props.symbols.refresh();
  }

  return (
    <main class="firmware-symbols" hidden={!props.shown} aria-labelledby="symbols-settings">
      <header class="page-head">
        <h2 id="symbols-settings">Settings</h2>
      </header>
      <div class="symbols-body">
        <section aria-labelledby="firmware-symbols">
          <h3 id="firmware-symbols">Firmware symbols</h3>
          <p>Matching symbols give firmware addresses names in the workbench.</p>
          <p class="symbols-target" role="status">
            <strong>{props.targetName}</strong>
            <span>{imageStatus()}</span>
            <Show when={activeHash()}>
              {(hash) => (
                <code class="selectable" title="Active firmware SHA-256">
                  {hash()}
                </code>
              )}
            </Show>
          </p>
          <div class="symbols-actions">
            <For each={kinds}>
              {({ kind, label }) => (
                <button type="button" disabled={busy()} onClick={() => void run(() => props.symbols.choose(kind))}>
                  Add {label}…
                </button>
              )}
            </For>
            <button type="button" disabled={busy()} onClick={() => void run(refresh)}>
              Refresh sources
            </button>
          </div>
          <Show when={actionError() || sources.latest?.error}>
            <p class="symbols-error" role="alert">
              {actionError() || sources.latest?.error}
            </p>
          </Show>
          <Show when={sources.loading && !sources.latest}>
            <p role="status">Reading sources…</p>
          </Show>
          <Show when={sources.latest && !sources.latest.error}>
            <Show when={sources.latest?.files.length} fallback={<p>No symbol sources added.</p>}>
              <ul class="symbols-files" aria-label="Symbol sources" aria-busy={busy()}>
                <For each={sources.latest?.files}>
                  {(source) => (
                    <li>
                      <div class="symbols-file">
                        <div>
                          <strong>{kinds.find(({ kind }) => kind === source.kind)?.label}</strong>
                          <code class="selectable">{source.path}</code>
                        </div>
                        <button
                          type="button"
                          disabled={busy()}
                          aria-label={`Remove ${source.path}`}
                          onClick={() => void run(() => props.symbols.remove(source.path))}
                        >
                          Remove
                        </button>
                      </div>
                      <Show when={source.error}>
                        <p class="symbols-error">{source.error}</p>
                      </Show>
                      <ul class="symbols-images" aria-label="Firmware images">
                        <For each={source.images}>
                          {(entry) => (
                            <li>
                              {deviceNameOf(entry.device)} · OS {entry.firmware}
                              <Show when={!source.error && entry.sha256.toLowerCase() === activeHash()}>
                                <span class="symbols-match">Matches target</span>
                              </Show>
                              <code class="selectable" title="Firmware image SHA-256">
                                {entry.sha256}
                              </code>
                            </li>
                          )}
                        </For>
                      </ul>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </section>
      </div>
    </main>
  );
}

import { For, Show, createMemo, createResource, createSignal } from 'solid-js';
import { deviceNameOf } from '@digi/workbench-core/data/device-profiles.ts';
import type { FirmwareProfile, ProfileSymbolKind, SymbolsHost } from '@digi/workbench-core/host/host.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import './firmware-symbols.css';

type SourceKind = Parameters<SymbolsHost['choose']>[0];

const kinds: { kind: SourceKind; label: string }[] = [
  { kind: 'cf', label: 'ColdFire database' },
  { kind: 'sharc', label: 'SHARC database' },
  { kind: 'map', label: 'Symbol map' },
];

const profileKinds: { kind: ProfileSymbolKind; label: string }[] = [
  { kind: 'cf', label: 'ColdFire database' },
  { kind: 'sharc', label: 'SHARC database' },
  { kind: 'catalog', label: 'SHARC catalog' },
];

const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error));

export default function FirmwareSymbols(props: {
  shown: boolean;
  symbols: SymbolsHost;
  source?: TargetBinding;
  imageRevision?: unknown;
  imageFor(source: TargetBinding): Promise<string | undefined>;
  targetName: string;
  openCatalog?(sha256: string): void;
}) {
  const [working, setWorking] = createSignal(false);
  const [actionError, setActionError] = createSignal('');
  const [refreshCount, setRefreshCount] = createSignal(0);
  const [sources, { refetch }] = createResource(
    () => props.shown,
    async () => {
      try {
        const [files, profiles] = await Promise.all([props.symbols.list(), props.symbols.profiles?.() ?? []]);
        return { files, profiles, error: '' };
      } catch (error) {
        return { files: [], profiles: [], error: errorMessage(error) };
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
  const extraSources = () => {
    const managed = new Set(sources.latest?.profiles.map(({ sha256 }) => sha256));
    return (
      sources.latest?.files.filter(
        (file) => file.kind === 'map' || !file.images.length || file.images.some(({ sha256 }) => !managed.has(sha256)),
      ) ?? []
    );
  };
  const matched = () =>
    sources.latest?.profiles.some(
      ({ sha256, sources }) => sha256 === activeHash() && Object.values(sources).some((row) => !row.error),
    ) ||
    extraSources().some(
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
    <div class="firmware-symbols" hidden={!props.shown}>
      <div class="symbols-body">
        <section aria-labelledby="firmware-symbols">
          <h3 id="firmware-symbols">Firmware symbols</h3>
          <p>Workbench selects these files by the firmware hash of the target or recording.</p>
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
                <Show when={kind === 'map' || !props.symbols.profiles}>
                  <button type="button" disabled={busy()} onClick={() => void run(() => props.symbols.choose(kind))}>
                    Add {label}…
                  </button>
                </Show>
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
            <div class="symbol-profiles">
              <For each={sources.latest?.profiles}>
                {(profile) => (
                  <ReleaseSettings
                    profile={profile}
                    matches={profile.sha256 === activeHash()}
                    busy={busy()}
                    choose={(kind) => void run(() => props.symbols.chooseProfile!(profile.sha256, kind))}
                    reset={(kind) => void run(() => props.symbols.resetProfile!(profile.sha256, kind))}
                    openCatalog={props.openCatalog ? () => props.openCatalog!(profile.sha256) : undefined}
                  />
                )}
              </For>
            </div>
            <Show
              when={extraSources().length}
              fallback={props.symbols.profiles ? undefined : <p>No symbol sources added.</p>}
            >
              <Show when={props.symbols.profiles}>
                <h4>Additional symbol sources</h4>
              </Show>
              <ul class="symbols-files" aria-label="Symbol sources" aria-busy={busy()}>
                <For each={extraSources()}>
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
    </div>
  );
}

function ReleaseSettings(props: {
  profile: FirmwareProfile;
  matches: boolean;
  busy: boolean;
  choose(kind: ProfileSymbolKind): void;
  reset(kind: ProfileSymbolKind): void;
  openCatalog?(): void;
}) {
  return (
    <details class="symbol-profile" open>
      <summary>
        <strong>
          {deviceNameOf(props.profile.device)} · OS {props.profile.firmware}
        </strong>
        <Show when={props.matches}>
          <span class="symbols-match">Matches target</span>
        </Show>
      </summary>
      <ul class="symbols-files" aria-label={`${deviceNameOf(props.profile.device)} firmware files`}>
        <For each={profileKinds}>
          {({ kind, label }) => {
            const row = () => props.profile.sources[kind];
            return (
              <li>
                <div class="symbols-file">
                  <div>
                    <strong>{label}</strong>
                    <span class="symbols-origin">{row().override ? 'Custom' : 'Default'}</span>
                    <code class="selectable" title={row().path}>
                      {row().path}
                    </code>
                  </div>
                  <div class="symbols-actions">
                    <Show when={kind === 'catalog' && props.profile.review && props.openCatalog}>
                      <button type="button" onClick={props.openCatalog}>
                        Open catalog
                      </button>
                    </Show>
                    <button
                      type="button"
                      disabled={props.busy}
                      aria-label={`Choose ${label} for ${deviceNameOf(props.profile.device)}`}
                      onClick={() => props.choose(kind)}
                    >
                      Choose…
                    </button>
                    <Show when={row().override}>
                      <button type="button" disabled={props.busy} onClick={() => props.reset(kind)}>
                        Use default
                      </button>
                    </Show>
                  </div>
                </div>
                <Show when={row().error}>
                  <p class="symbols-error">{row().error}</p>
                </Show>
              </li>
            );
          }}
        </For>
      </ul>
    </details>
  );
}

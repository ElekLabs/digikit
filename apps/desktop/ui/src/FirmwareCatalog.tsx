import { For, Show, createMemo, createResource, createSignal, onCleanup, onMount } from 'solid-js';
import { deviceNameOf } from '@digi/workbench-core/data/device-profiles.ts';
import type { FirmwareProfile, SymbolsHost } from '@digi/workbench-core/host/host.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import './firmware-catalog.css';

export interface CatalogSelection {
  sha256: string;
  entry?: number;
  path?: string;
}

const label = (profile: FirmwareProfile) => deviceNameOf(profile.device) + ' · OS ' + profile.firmware;
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

export default function FirmwareCatalog(props: {
  symbols: SymbolsHost;
  selection?: CatalogSelection;
  source?: TargetBinding;
  imageRevision?: unknown;
  imageFor(source: TargetBinding): Promise<string | undefined>;
  select(selection: CatalogSelection): void;
  settings(): void;
  back?(): void;
}) {
  let frame: HTMLIFrameElement | undefined;
  const [actionError, setActionError] = createSignal('');
  const [state] = createResource(() => props.symbols.profiles?.() ?? Promise.resolve([]));
  const imageRequest = createMemo(() => {
    const source = props.source;
    if (props.selection || !source || source.target.kind === 'none') return;
    return { source, revision: props.imageRevision };
  });
  const [activeImage] = createResource(imageRequest, async (request) => {
    const hash = await props.imageFor(request.source).catch(() => undefined);
    return { request, hash };
  });
  const profiles = () => (state.error ? [] : (state.latest ?? []));
  const hash = () => {
    if (props.selection) return props.selection.sha256;
    if (activeImage.loading) return;
    const active = activeImage.latest;
    const matched = active?.request === imageRequest() ? active?.hash : undefined;
    return (
      profiles().find((profile) => profile.sha256 === matched)?.sha256 ??
      profiles().find((profile) => profile.review)?.sha256 ??
      profiles()[0]?.sha256
    );
  };
  const selection = createMemo(() => {
    const sha256 = hash();
    if (!sha256 || state.loading) return;
    const profile = profiles().find((profile) => profile.sha256 === sha256);
    const path = profile ? profile.review?.path : props.selection?.path;
    if (path) return { path, sha256, entry: props.selection?.entry };
  });

  const [view] = createResource(selection, async (selected) => {
    setActionError('');
    const url = await props.symbols.catalogFrame!(selected.path, selected.sha256, selected.entry);
    return { selected, url };
  });
  const currentView = () => {
    const current = view.loading ? undefined : view();
    return current?.selected === selection() ? current : undefined;
  };

  function followPair(event: MessageEvent) {
    const data = event.data;
    if (!frame || event.source !== frame.contentWindow || data?.kind !== 'firmware-catalog-pair') return;
    if (!Number.isSafeInteger(data.entry) || data.entry < 0 || data.entry > 0xffffffff) return;
    const destination = profiles().find((profile) => profile.device + '-' + profile.firmware === data.release);
    if (!destination?.review?.entries.includes(data.entry)) {
      setActionError('The paired function is unavailable in the configured catalog.');
      return;
    }
    props.select({ sha256: destination.sha256, entry: data.entry });
  }
  onMount(() => window.addEventListener('message', followPair));
  onCleanup(() => window.removeEventListener('message', followPair));

  async function openInBrowser() {
    const current = selection();
    if (!current) return;
    setActionError('');
    try {
      await props.symbols.openReview?.(current.path, current.sha256, current.entry);
    } catch (error) {
      setActionError(message(error));
    }
  }

  return (
    <section class="firmware-catalog" aria-label="Firmware catalog">
      <header class="firmware-catalog-controls">
        <Show when={props.back}>
          <button type="button" onClick={props.back}>
            Back to profile
          </button>
        </Show>
        <label>
          Firmware
          <select
            value={hash() ?? ''}
            onChange={(event) => {
              const sha256 = event.currentTarget.value;
              if (sha256) props.select({ sha256 });
            }}
          >
            <Show when={!profiles().some((profile) => profile.sha256 === hash())}>
              <option value={hash() ?? ''}>{props.selection?.path ? 'Recorded firmware' : 'Choose firmware'}</option>
            </Show>
            <For each={profiles()}>
              {(profile) => (
                <option value={profile.sha256} selected={profile.sha256 === hash()}>
                  {label(profile)}
                </option>
              )}
            </For>
          </select>
        </label>
        <button type="button" onClick={props.settings}>
          Firmware settings
        </button>
        <Show when={selection() && props.symbols.openReview}>
          <button type="button" onClick={() => void openInBrowser()}>
            Open in browser
          </button>
        </Show>
      </header>
      <Show when={actionError() || state.error || view.error}>
        <p class="catalog-error" role="alert">
          {actionError() || message(state.error ?? view.error)}
        </p>
      </Show>
      <Show when={state.loading || activeImage.loading || view.loading}>
        <p role="status">Loading catalog…</p>
      </Show>
      <Show when={!state.loading && !activeImage.loading && !state.error && !selection()}>
        <p>No catalog for this firmware. Generate a SHARC review or choose its HTML file in Firmware settings.</p>
      </Show>
      <Show when={!view.error && currentView()} keyed>
        {(current) => <iframe ref={frame} title="SHARC review" sandbox="allow-scripts" src={current.url} />}
      </Show>
    </section>
  );
}

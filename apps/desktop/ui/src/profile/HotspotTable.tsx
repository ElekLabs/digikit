import { createSignal, For, Show, type JSX } from 'solid-js';
import { fmtPct, hotspotLabel, type Hotspot } from './hotspots';

export default function HotspotTable(props: {
  title: string;
  rows: Hotspot[];
  openReview?(path: string, sha256: string, entry: number): Promise<void>;
}): JSX.Element {
  const [error, setError] = createSignal('');
  async function open(row: Hotspot) {
    const link = row.review;
    if (!link || !props.openReview) return;
    setError('');
    try {
      await props.openReview(link.path, link.sha256, link.entry);
    } catch (err) {
      setError(String(err));
    }
  }
  return (
    <div class="cf-paths" role="table" aria-label={props.title}>
      <div class="cf-row head" role="row">
        <span class="name">{props.title} by samples</span>
        <span class="mono">share</span>
      </div>
      <For each={props.rows}>
        {(row) => (
          <div class="cf-row" role="row">
            <Show
              when={props.openReview && row.review?.url}
              fallback={
                <Show
                  when={row.module_entry !== undefined && row.entry !== undefined}
                  fallback={
                    <span class="name" title={hotspotLabel(row)}>
                      {hotspotLabel(row)}
                    </span>
                  }
                >
                  <details class="name">
                    <summary title={hotspotLabel(row)}>{hotspotLabel(row)}</summary>
                    <div>{row.owner}</div>
                    <div>
                      Entry 0x{row.entry!.toString(16)} · module offset 0x{row.module_entry!.toString(16)}
                    </div>
                    <div>
                      {row.weight} {row.unit ?? 'samples'}
                    </div>
                  </details>
                </Show>
              }
            >
              <a
                class="name catalog-link"
                href={row.review && `${row.review.url}#${row.review.fragment}`}
                title="Open function in firmware catalog"
                onClick={(event) => {
                  event.preventDefault();
                  void open(row);
                }}
              >
                {hotspotLabel(row)}
              </a>
            </Show>
            <span class="mono">{fmtPct(row.share)}</span>
          </div>
        )}
      </For>
      <Show when={error()}>
        <p class="err" role="alert">
          {error()}
        </p>
      </Show>
    </div>
  );
}

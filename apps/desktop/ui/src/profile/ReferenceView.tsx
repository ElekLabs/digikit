import { For, Show, type JSX } from 'solid-js';
import HotspotTable from './HotspotTable';
import type { SavedEvidence } from './hotspots';
import { processorLabels } from '@digi/workbench-core/profile/processor.ts';

export function ReferenceView(props: {
  name?: string;
  evidence?: SavedEvidence;
  openReview?(path: string, sha256: string, entry: number): Promise<void>;
}): JSX.Element {
  return (
    <>
      <Show when={props.evidence} keyed>
        {({ pcs, sharc }) => (
          <Show
            when={pcs.cf.length || pcs.sharc.length}
            fallback={<p class="empty">{props.name} has no sampled program counters.</p>}
          >
            <div class="prof-sec ruled">
              <div class="sec-head">
                <span class="title" title={props.name}>
                  Saved hotspots · {props.name}
                </span>
              </div>
              <Show when={sharc}>
                {(costs) => (
                  <>
                    <SharcCostsTable
                      title="DSP functions"
                      unit={costs().unit}
                      rows={costs().functions}
                      label={(row) => row.name}
                      detail={(row) => row.module ?? row.layer}
                    />
                    <SharcCostsTable
                      title="DSP loops"
                      unit={costs().unit}
                      rows={costs().loops ?? []}
                      label={(row) => row.function ?? row.module}
                      detail={(row) => `${row.module} · +0x${row.start.toString(16)} · ${row.count}`}
                    />
                  </>
                )}
              </Show>
              <Show when={pcs.cf.length}>
                <HotspotTable title={`${processorLabels.main} PCs`} rows={pcs.cf} openReview={props.openReview} />
              </Show>
              <Show when={pcs.sharc.length}>
                <HotspotTable title={`${processorLabels.dsp} PCs`} rows={pcs.sharc} openReview={props.openReview} />
                <Show
                  when={
                    props.openReview &&
                    pcs.sharc.some((row) => row.owner === 'firmware') &&
                    !pcs.sharc.some((row) => row.review?.url)
                  }
                >
                  <div class="hint">
                    No catalog links for these hotspots. Generate the DSP review for this firmware and refresh Firmware
                    symbols in Settings. Links appear for functions included in the review.
                  </div>
                </Show>
              </Show>
            </div>
          </Show>
        )}
      </Show>
    </>
  );
}

function SharcCostsTable<Row extends { per_block?: number; self_count: number }>(props: {
  title: string;
  unit: string;
  rows: readonly Row[];
  label(row: Row): string;
  detail(row: Row): string | undefined;
}): JSX.Element {
  const perBlock = () => props.rows[0]?.per_block !== undefined;
  return (
    <Show when={props.rows.length}>
      <div class="cf-paths" role="table" aria-label={props.title}>
        <div class="cf-row head" role="row">
          <span class="name">{props.title}</span>
          <span class="mono">
            {props.unit}
            {perBlock() ? ' / block' : ''}
          </span>
        </div>
        <For each={props.rows}>
          {(row) => (
            <div class="cf-row" role="row">
              <span class="name" title={props.detail(row)}>
                {props.label(row)}
              </span>
              <span class="mono">
                {(row.per_block ?? row.self_count).toLocaleString(undefined, { maximumFractionDigits: 1 })}
              </span>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
}

import { For, type JSX } from 'solid-js';
import { fmtPct, hotspotLabel, type Hotspot } from './hotspots';

export default function HotspotTable(props: { title: string; rows: Hotspot[] }): JSX.Element {
  return (
    <div class="cf-paths" role="table" aria-label={props.title}>
      <div class="cf-row head" role="row">
        <span class="name">{props.title} by samples</span>
        <span class="mono">share</span>
      </div>
      <For each={props.rows}>
        {(row) => (
          <div class="cf-row" role="row">
            <span class="name" title={hotspotLabel(row)}>
              {hotspotLabel(row)}
            </span>
            <span class="mono">{fmtPct(row.share)}</span>
          </div>
        )}
      </For>
    </div>
  );
}

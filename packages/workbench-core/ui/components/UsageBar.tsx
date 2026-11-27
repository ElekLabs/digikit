// The usage bar of a store, like a partition manager: coloured parts in a row and a legend. The Files pane and the Transfer storage bar draw it.
import { For, type JSX } from 'solid-js';
import './usage-bar.css';

export interface UsagePart {
  /** The colour class (`seg-<key>`): a store category, `overhead`, `others`, `preview` or `free`. */
  key: string;
  bytes: number;
  /** The tooltip of the part on the bar. */
  title: string;
  /** The legend line. */
  legend: string;
}

export default function UsageBar(props: {
  parts: UsagePart[];
  /** The size the parts are measured against. */
  base: number;
  label: string;
  /** More legend lines without a colour (the total). */
  notes?: string[];
  /** The bar is over its limit. */
  over?: boolean;
}): JSX.Element {
  const pct = (bytes: number): string => `${props.base > 0 ? (100 * bytes) / props.base : 0}%`;
  return (
    <div class="usagebox">
      <div class="usage" classList={{ over: props.over }} role="img" aria-label={props.label}>
        <For each={props.parts}>
          {(x) => <span class={`seg seg-${x.key}`} style={{ 'flex-basis': pct(x.bytes) }} title={x.title} />}
        </For>
      </div>
      <ul class="legend">
        <For each={props.parts}>
          {(x) => (
            <li>
              <i class={`sw seg-${x.key}`} />
              {x.legend}
            </li>
          )}
        </For>
        <For each={props.notes ?? []}>{(n) => <li>{n}</li>}</For>
      </ul>
    </div>
  );
}

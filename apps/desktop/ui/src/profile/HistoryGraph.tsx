import { For, createMemo, type JSX } from 'solid-js';
import {
  fmtShare,
  graphTop,
  STACK,
  type Column,
  type Marker,
  type OwnerKind,
} from '@digi/workbench-core/profile/profile-core.ts';

const LEGEND: { kind: OwnerKind; text: string }[] = [
  { kind: 'machine', text: 'machine' },
  { kind: 'effect', text: 'effect' },
  { kind: 'tool', text: 'tool' },
  { kind: 'runtime', text: 'runtime' },
];
const COLUMN_PX = 4;
const GAP_PX = 2;
const GRAPH_PX = 90;

export default function HistoryGraph(props: {
  columns: Column[];
  markers: Marker[];
  emulator: boolean;
  share: number;
}): JSX.Element {
  const columns = createMemo(() => props.columns);
  const top = createMemo(() => graphTop(columns()));
  const marks = createMemo(() => props.markers.slice(-3));
  const px = (share: number) => Math.round((share / top()) * GRAPH_PX);

  return (
    <div class="prof-sec">
      <div class="sec-head">
        <span class="title">{props.emulator ? 'ColdFire, last 60 s' : 'ColdFire, recorded window'}</span>
        <span class="num">
          {fmtShare(props.share)} {props.emulator ? 'now' : 'mean'}
        </span>
      </div>
      <div class="graph" role="img" aria-label="ColdFire share per owner type over time">
        <For each={columns()}>
          {(c) => (
            <div class="col">
              <For each={STACK}>{(k) => <div class={`k-${k}`} style={{ height: `${px(c[k])}px` }} />}</For>
            </div>
          )}
        </For>
        <For each={marks()}>
          {(m) => (
            <>
              <div class="mark" style={{ left: `${6 + m.col * (COLUMN_PX + GAP_PX)}px` }} />
              <span class="mark-text" style={{ left: `${10 + m.col * (COLUMN_PX + GAP_PX)}px` }}>
                {m.text}
              </span>
            </>
          )}
        </For>
        <span class="top">{Math.round(top() * 100)}%</span>
      </div>
      <ul class="legend" aria-label="Owner types">
        <For each={LEGEND}>
          {(l) => (
            <li>
              <span class={`swatch k-${l.kind}`} />
              {l.text}
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}

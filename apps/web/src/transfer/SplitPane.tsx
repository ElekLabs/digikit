// Two panes side by side with a draggable divider between them (Transfer: "On your computer" | "On device"). The first two children are the
// panes. The divider moves with the pointer (or the arrow keys) between two minimum widths; a double click puts it back in the middle-ish
// default. The position is remembered per viewer in the app's KeyValue under `id` (no storage: it lasts until the window closes).
import { children, createSignal, type JSX } from 'solid-js';
import { useTransferApp } from './app-context';
import { KEY_STEP, SPLIT_DEFAULT, clampSplit, parseSplit, splitAt } from './split-core';
import './split-pane.css';

const KEY = (id: string) => `digikit.split.${id}`;

export default function SplitPane(props: { id: string; children: JSX.Element }): JSX.Element {
  let box: HTMLDivElement | undefined;
  const { storage } = useTransferApp();
  const [split, setSplit] = createSignal(parseSplit(storage.get(KEY(props.id))));
  const [dragging, setDragging] = createSignal(false);
  const kids = children(() => props.children);
  const panes = () => kids.toArray();

  const keep = () => storage.set(KEY(props.id), String(split()));
  const width = () => box?.getBoundingClientRect().width ?? 0;
  const move = (e: PointerEvent) => {
    if (!box || !dragging()) return;
    const r = box.getBoundingClientRect();
    setSplit(splitAt(e.clientX, r.left, r.width));
  };
  const nudge = (delta: number) => {
    setSplit(clampSplit(split() + delta, width()));
    keep();
  };

  return (
    <div ref={box} class="split" classList={{ dragging: dragging() }} style={{ '--split': String(split()) }}>
      {panes()[0]}
      <div
        class="split-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the panes"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(split() * 100)}
        tabIndex={0}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragging(true);
        }}
        onPointerMove={move}
        onPointerUp={() => {
          if (dragging()) keep();
          setDragging(false);
        }}
        onPointerCancel={() => setDragging(false)}
        onDblClick={() => {
          setSplit(clampSplit(SPLIT_DEFAULT, width()));
          keep();
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowLeft') nudge(-KEY_STEP);
          else if (e.key === 'ArrowRight') nudge(KEY_STEP);
          else return;
          e.preventDefault();
        }}
      />
      {panes()[1]}
    </div>
  );
}

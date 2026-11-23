import { Show, createSignal, onCleanup, onMount, untrack, type JSX } from 'solid-js';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import { DOCK_KEY_STEP, DOCK_MIN_PX, clampDock, dockAt, dockMax, parseDock, startDock } from './dock-core';
import type { DrawerName } from './log-state';

const DOCK_KEY = 'digikit.logHeight';

export default function WorkbenchDrawer(props: {
  active: DrawerName | undefined;
  storage: KeyValue;
  initialHeight?: number;
  onDragging(dragging: boolean): void;
  children: JSX.Element;
}) {
  const [saved, setSaved] = createSignal(untrack(() => props.initialHeight ?? parseDock(props.storage.get(DOCK_KEY))));
  const [windowPx, setWindowPx] = createSignal(window.innerHeight);
  const [dragging, setDragging] = createSignal(false);
  let dock: HTMLElement | undefined;

  const height = () => startDock(saved(), windowPx());
  const persist = () => {
    const px = saved();
    if (px === undefined) props.storage.remove(DOCK_KEY);
    else props.storage.set(DOCK_KEY, String(px));
  };
  const drag = (on: boolean) => {
    setDragging(on);
    props.onDragging(on);
  };
  const nudge = (delta: number) => {
    setSaved(clampDock(height() + delta, windowPx()));
    persist();
  };
  const onResize = () => setWindowPx(window.innerHeight);
  onMount(() => window.addEventListener('resize', onResize));
  onCleanup(() => window.removeEventListener('resize', onResize));

  return (
    <Show when={props.active}>
      <section
        class="workbench-drawer"
        classList={{ 'log-drawer': props.active === 'Log' }}
        aria-label={props.active}
        ref={dock}
        style={{ height: `${height()}px` }}
      >
        <div
          class="dock-handle"
          role="separator"
          aria-orientation="horizontal"
          aria-label={`Resize the ${props.active}`}
          aria-valuemin={DOCK_MIN_PX}
          aria-valuemax={dockMax(windowPx())}
          aria-valuenow={height()}
          tabIndex={0}
          onPointerDown={(e) => {
            if (e.button !== 0) return;
            e.currentTarget.setPointerCapture(e.pointerId);
            drag(true);
          }}
          onPointerMove={(e) => {
            if (dragging() && dock) setSaved(dockAt(e.clientY, dock.getBoundingClientRect().bottom, windowPx()));
          }}
          onPointerUp={() => {
            if (dragging()) persist();
            drag(false);
          }}
          onPointerCancel={() => drag(false)}
          onDblClick={() => {
            setSaved(undefined);
            persist();
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowUp') nudge(DOCK_KEY_STEP);
            else if (e.key === 'ArrowDown') nudge(-DOCK_KEY_STEP);
            else return;
            e.preventDefault();
          }}
        />
        {props.children}
      </section>
    </Show>
  );
}

// The selected target’s inspector. Each binding owns its panel reads and calibration run.
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { For, Show, createEffect, createSignal, on, type JSX } from 'solid-js';
import type { Host } from '@digi/workbench-core/host/host.ts';
import MemoryPanel from './MemoryPanel';
import type { SpanRecording } from './span-recording';
import ProfilePanel from './ProfilePanel';
import CalibrationPanel from './CalibrationPanel';
import { createCalibrationHub } from './calibration-hub';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { TAB_LABEL, shownTab, type InspectorTab } from './inspector-core';
import { inspectorOpen, inspectorTab, setInspectorOpen, setInspectorTab } from './panel-open';
import './card-panel.css';

export default function SidePanels(props: {
  /** The host of the Profile panel (the emulator's `cf.profile` and `sharc.profile`, the save dialog). */
  host?: Host;
  visible: boolean;
  active?: boolean;
  collectSpans?: boolean;
  liveDsp?: boolean;
  profileGeneration?: number;
  onSpanRecording?(recording?: SpanRecording): void;
  metrics?: JSX.Element;
  source: TargetBinding;
}): JSX.Element {
  // shared by the Profile and Calibration tabs: a run in progress, and the calibration kept per device
  const hub = createCalibrationHub(useTransferApp().storage);
  const offered = (): InspectorTab[] => [
    'memory',
    ...(props.metrics ? (['metrics'] as const) : []),
    'profile',
    ...(props.host ? (['calibration'] as const) : []),
  ];
  const tab = () => shownTab(inspectorTab(), offered());
  const reads = (t: InspectorTab) => props.visible && (props.active ?? true) && inspectorOpen() && tab() === t;
  const tabpanel = (t: InspectorTab) =>
    offered().length > 1 ? { id: `side-panel-${t}`, labelledBy: `side-tab-${t}` } : undefined;
  const pick = (t: InspectorTab, focus = false) => {
    // a column with one tab must not overwrite the choice made on the Emulator page
    if (offered().length > 1) setInspectorTab(t);
    setInspectorOpen(true);
    if (focus) requestAnimationFrame(() => document.getElementById(`side-tab-${t}`)?.focus());
  };
  const onTabKey = (e: KeyboardEvent) => {
    const list = offered();
    const i = list.indexOf(tab());
    let next: InspectorTab;
    if (e.key === 'ArrowRight') next = list[(i + 1) % list.length]!;
    else if (e.key === 'ArrowLeft') next = list[(i - 1 + list.length) % list.length]!;
    else if (e.key === 'Home') next = list[0]!;
    else if (e.key === 'End') next = list[list.length - 1]!;
    else return;
    e.preventDefault();
    pick(next, true);
  };
  // the line under the chosen tab slides to it
  let tabsEl: HTMLDivElement | undefined;
  const [ink, setInk] = createSignal({ x: 0, w: 0 });
  // measured again when the column shows (a hidden column measures 0)
  createEffect(
    on([tab, inspectorOpen, () => offered().length, () => props.visible], () =>
      requestAnimationFrame(() => {
        const chosen = tabsEl?.querySelector<HTMLElement>('.side-tab.on');
        if (chosen) setInk({ x: chosen.offsetLeft, w: chosen.offsetWidth });
      }),
    ),
  );
  return (
    <div
      class="side-col"
      classList={{ rail: !inspectorOpen(), wide: inspectorOpen() && (tab() === 'profile' || tab() === 'calibration') }}
      hidden={!props.visible}
    >
      <div class="side-head">
        <Show when={inspectorOpen()}>
          <Show
            when={offered().length > 1}
            fallback={
              <span class="side-title" id={`side-tab-${tab()}`}>
                {TAB_LABEL[tab()]}
              </span>
            }
          >
            <div class="side-tabs" role="tablist" aria-label="Inspector" ref={tabsEl} onKeyDown={onTabKey}>
              <For each={offered()}>
                {(t) => (
                  <button
                    type="button"
                    role="tab"
                    id={`side-tab-${t}`}
                    aria-controls={`side-panel-${t}`}
                    aria-selected={tab() === t}
                    tabindex={tab() === t ? 0 : -1}
                    class="side-tab"
                    classList={{ on: tab() === t }}
                    onClick={() => pick(t)}
                  >
                    {TAB_LABEL[t]}
                  </button>
                )}
              </For>
              <span
                class="side-ink"
                aria-hidden="true"
                style={{ transform: `translateX(${ink().x}px)`, width: `${ink().w}px` }}
              />
            </div>
          </Show>
        </Show>
        <span class="sp" />
        <button
          type="button"
          class="side-fold"
          aria-expanded={inspectorOpen()}
          aria-label={inspectorOpen() ? 'Fold the column' : 'Open the column'}
          title={inspectorOpen() ? 'Fold the column' : `Open the column (${TAB_LABEL[tab()]})`}
          onClick={() => {
            const opening = !inspectorOpen();
            setInspectorOpen(opening);
            if (opening) requestAnimationFrame(() => document.getElementById(`side-tab-${tab()}`)?.focus());
          }}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="M6 3L11 8L6 13"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </button>
      </div>
      <Show when={!inspectorOpen()}>
        <div class="side-rail-tabs">
          <For each={offered()}>
            {(t) => (
              <button
                type="button"
                class="side-rail-tab"
                classList={{ on: tab() === t }}
                title={`Open ${TAB_LABEL[t]}`}
                onClick={() => pick(t, true)}
              >
                {TAB_LABEL[t]}
              </button>
            )}
          </For>
        </div>
      </Show>
      <Show when={props.visible && inspectorOpen() && !(props.active ?? true) && tab() !== 'metrics'}>
        <p class="side-empty">The emulator is not running.</p>
      </Show>
      <MemoryPanel visible={reads('memory')} source={props.source} tabpanel={tabpanel('memory')} />
      <Show when={props.profileGeneration ?? 1} keyed>
        {(_generation) => (
          <ProfilePanel
            visible={reads('profile')}
            liveDsp={props.liveDsp}
            collectSpans={props.collectSpans && (props.active ?? true)}
            onSpanRecording={props.onSpanRecording}
            source={props.source}
            host={props.host}
            hub={hub}
            tabpanel={tabpanel('profile')}
          />
        )}
      </Show>
      <Show when={props.host}>
        <CalibrationPanel
          visible={reads('calibration')}
          source={props.source}
          host={props.host}
          hub={hub}
          tabpanel={tabpanel('calibration')}
        />
      </Show>
      <Show when={props.metrics}>
        <div
          class="card-panel metrics-panel"
          role="tabpanel"
          id="side-panel-metrics"
          aria-labelledby="side-tab-metrics"
          hidden={!(props.visible && inspectorOpen() && tab() === 'metrics')}
        >
          {props.metrics}
        </div>
      </Show>
    </div>
  );
}

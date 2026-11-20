// The Calibration tab of the right-hand column (SidePanels.tsx): the run, fit and save section (CalibrationSection.tsx) and the active calibration,
// a dk.calibration/1 file kept per device that the Profile tab estimates DSP cycles with. The pane owns the run state (calibration-state.ts), so
// switching tabs never aborts a run; the Profile tab only learns through the hub (calibration-hub.ts) that a run is playing. The pane reads nothing
// itself: it asks the target's INFO once (when shown or when the target changes) for the device the active calibration belongs to.
import { Show, createEffect, createSignal, onCleanup, untrack, type JSX } from 'solid-js';
import { panels } from '@digi/workbench-core/data/faceplates.ts';
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { DkSession } from '@digi/dk-session';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { sameTarget, type Target } from '@digi/workbench-core/transfer/target.ts';
import { cardRefresh } from './card-state';
import { createDkProfLoader } from './dkprof-env';
import { deviceOfPort } from './remote-core';
import CalibrationSection from './CalibrationSection';
import { calibrationTexts } from './calibration-data';
import { createCalibrator } from './calibration-backend';
import { createCalibrationState } from './calibration-state';
import type { CalibrationHub } from './calibration-hub';
import './card-panel.css';
import './profile-panel.css';

const asText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function CalibrationPanel(props: {
  visible: boolean;
  session: () => Promise<DkSession>;
  where: () => string;
  track?: () => unknown;
  host?: Host;
  hub: CalibrationHub;
  tabpanel?: { id: string; labelledBy: string };
}): JSX.Element {
  const app = useTransferApp();
  const { emulatorRuntime, target } = app.target;
  const readsEmulator = () => props.where() === 'the emulator';
  const readsTarget = () => !readsEmulator() || target().kind === 'emulator';
  const onEmulator = () => readsEmulator() || target().kind === 'emulator';
  const prof = props.host ? createDkProfLoader(props.host) : undefined;

  // the device of the target, from its INFO port (`<id>-<version>`)
  const [port, setPort] = createSignal('');
  let latest: object | undefined;
  createEffect(() => {
    cardRefresh();
    props.track?.();
    emulatorRuntime();
    if (!props.visible) return;
    untrack(() => {
      const mine = {};
      latest = mine;
      void props
        .session()
        .then((s) => s.info())
        .then((i) => latest === mine && setPort(i.port))
        .catch(() => latest === mine && setPort(''));
    });
  });
  const device = () => deviceOfPort(port());
  const deviceName = () => {
    const d = device();
    const version = port().slice(port().indexOf('-') + 1);
    return d ? `${panels[d].name}${version && version !== port() ? ` ${version}` : ''}` : '';
  };

  // the run fails when the selected target changes under it
  let runTarget: Target | undefined;
  const sameRunTarget = () => {
    if (readsTarget() && runTarget && !sameTarget(runTarget, target()))
      throw new Error('the target changed during the run: it was stopped');
  };
  const calibrator = createCalibrator({
    session: async () => props.session(),
    get rpc() {
      return onEmulator() ? props.host?.rpc : undefined;
    },
    get midi() {
      const sink = props.host?.ports.midi;
      return sink
        ? {
            send: async (bytes: Uint8Array) => {
              sameRunTarget();
              await sink.send(bytes);
            },
          }
        : undefined;
    },
    loadLibrary: app.loadLibrary,
    // Transfer installs on the selected target: only when this pane reads it, and only while it is still the target the run began on
    get install() {
      return readsTarget()
        ? async (ids: string[]) => {
            sameRunTarget();
            await app.backend.install(ids, () => {});
          }
        : undefined;
    },
    prof: prof ?? (async () => Promise.reject(new Error('dkprof.wasm needs the desktop host'))),
    texts: calibrationTexts,
  });
  const calibration = createCalibrationState({
    calibrator,
    emulator: onEmulator,
    blocked: () => false,
    onRunning: (r) => {
      runTarget = r ? target() : undefined;
      props.hub.setCalibrating(r);
    },
  });
  onCleanup(() => calibration.dispose());

  const active = () => props.hub.active.get(device());
  const [error, setError] = createSignal('');
  let loadInput: HTMLInputElement | undefined;
  const loadActive = async (file: File | undefined) => {
    if (!file) return;
    try {
      const refused = props.hub.active.load(file.name, await file.text(), device());
      setError(refused ?? '');
    } catch (e) {
      setError(`${file.name}: ${asText(e)}`);
    }
  };

  return (
    <aside
      class="card-panel profile-panel"
      id={props.tabpanel?.id}
      role={props.tabpanel ? 'tabpanel' : undefined}
      aria-labelledby={props.tabpanel?.labelledBy}
      aria-label={props.tabpanel ? undefined : 'Calibration'}
      hidden={!props.visible}
    >
      <div class="card-body">
        <div class="prof-sec cal-active">
          <div class="sec-head">
            <span class="title">Active calibration</span>
            <span class="num">{deviceName()}</span>
          </div>
          <div class="cal-bar">
            <span class="cal-target" classList={{ dim: !active() }}>
              {active() ? active()!.name : 'none'}
            </span>
            <button type="button" class="act quiet" disabled={calibration.running()} onClick={() => loadInput?.click()}>
              Load…
            </button>
            <button
              type="button"
              class="act quiet"
              disabled={!active()}
              onClick={() => {
                props.hub.active.clear(device());
                setError('');
              }}
            >
              Clear
            </button>
            <input
              ref={loadInput}
              type="file"
              hidden
              accept=".json,application/json"
              onChange={(e) => {
                void loadActive(e.currentTarget.files?.[0]);
                e.currentTarget.value = '';
              }}
            />
          </div>
          <Show when={error()}>
            <p class="err" role="alert">
              {error()}
            </p>
          </Show>
          <span class="hint">
            The Profile tab adds estimated device cycles to the DSP rows of the emulator with it. Kept for this device.
          </span>
        </div>
        <Show when={props.host}>
          <CalibrationSection
            state={calibration}
            host={props.host}
            target={`${onEmulator() ? 'Emulator' : 'USB'} ${deviceName()}`.trim()}
            emulator={onEmulator()}
          />
        </Show>
      </div>
    </aside>
  );
}

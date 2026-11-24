import { Show, untrack } from 'solid-js';
import { deviceNameOf, registry } from '@digi/workbench-core/data/device-profiles.ts';
import { audioProcessorLabel } from '../dsp-core';
import DefaultModules from '@digi/workbench-core/ui/firmware/DefaultModules.tsx';
import { CHART, FOLDER, Icon, MODULES, PAUSE, PLAY, RELOAD, RESTART } from '../icons';
import type { createEmulatorFirmware } from './firmware';

type ToolbarFirmware = Pick<
  ReturnType<typeof createEmulatorFirmware>,
  | 'status'
  | 'source'
  | 'watching'
  | 'loadOnStart'
  | 'bootRuntime'
  | 'runtimeHere'
  | 'runtimePending'
  | 'info'
  | 'bootDsp'
  | 'dspHere'
  | 'dspIsPending'
  | 'paused'
  | 'openFirmware'
  | 'toggleWatching'
  | 'chooseLoadOnStart'
  | 'chooseRuntime'
  | 'chooseDsp'
  | 'reload'
  | 'reloadModules'
>;

export default function FirmwareToolbar(props: {
  firmware: ToolbarFirmware;
  hidden: boolean;
  device: string;
  busy: boolean;
  reloadKey: string;
  modulesKey: string;
  canPause: boolean;
  canReset: boolean;
  onTogglePause(): void;
  onReset(): void;
  onSaveDiagnostics(): void;
}) {
  const {
    status,
    source,
    watching,
    loadOnStart,
    bootRuntime,
    runtimeHere,
    runtimePending,
    info,
    bootDsp,
    dspHere,
    dspIsPending,
    paused,
    openFirmware,
    toggleWatching,
    chooseLoadOnStart,
    chooseRuntime,
    chooseDsp,
    reload,
    reloadModules,
  } = untrack(() => props.firmware);

  const fileLabel = () => source()?.path || 'Open firmware…';
  /** The last three parts of the path; the full path is the tooltip. */
  const shortPath = () => {
    const parts = fileLabel().split('/');
    return parts.length > 3 ? `…/${parts.slice(-3).join('/')}` : fileLabel();
  };
  const processor = () =>
    audioProcessorLabel(registry.ids.includes(props.device) ? registry.get(props.device) : undefined);
  return (
    <header class="page-head emu-head" hidden={props.hidden}>
      <h2>Emulator</h2>
      <span class="file-chip" title={fileLabel()}>
        <button
          type="button"
          class="file-open"
          disabled={props.busy}
          aria-label="Open firmware file"
          title={source() ? 'Open another firmware file' : 'Open a firmware file'}
          onClick={() => void openFirmware()}
        >
          <Icon d={FOLDER} />
          <span class="path">{shortPath()}</span>
        </button>
        <Show when={status()}>
          <span class="tag device">
            {status()!.device} {status()!.version}
          </span>
        </Show>
        <Show when={source()}>
          <button
            type="button"
            class="tag watch"
            classList={{ off: !watching() }}
            aria-pressed={watching()}
            title="Reload when the file changes on disk"
            onClick={toggleWatching}
          >
            <i />
            {watching() ? 'watching' : 'not watching'}
          </button>
        </Show>
      </span>
      <span class="sp" />
      <label
        class="tag opt"
        title="Reopen the last opened firmware when Workbench starts (a command-line file takes precedence)"
      >
        <input type="checkbox" checked={loadOnStart()} onChange={(e) => chooseLoadOnStart(e.currentTarget.checked)} />
        <span class="long">Auto-load firmware</span>
        <span class="short">Auto-load</span>
      </label>
      <label
        class="tag opt"
        title={
          runtimeHere()
            ? 'At Open or Reload, Patchkit applies the digikit runtime and Workbench puts the system package on the card for module support.'
            : `${deviceNameOf(props.device)} has no digikit runtime: not available on this device`
        }
      >
        <input
          type="checkbox"
          checked={bootRuntime() && runtimeHere()}
          disabled={!runtimeHere()}
          onChange={(e) => chooseRuntime(e.currentTarget.checked)}
        />
        <span class="long">Boot with runtime</span>
        <span class="short">Runtime</span>
        <Show when={info()}>
          {(i) => (
            <span
              class="rt"
              classList={{ on: !!i().runtime?.patched, pending: runtimePending() }}
              title={
                runtimePending()
                  ? `The running image ${i().runtime ? 'has' : 'has no'} runtime; the tick applies at the next Reload (${props.reloadKey}) or Open`
                  : i().runtime?.patched
                    ? `${i().runtime?.modules?.length ?? 0} modules of the "${i().runtime?.set}" set, ${i().runtime?.cache_hit ? 'from the image cache' : 'patched now'}`
                    : i().runtime
                      ? 'The image has a patch record: it is loaded as it is'
                      : 'The image is loaded as it is'
              }
            >
              {runtimePending()
                ? 'on next Reload'
                : i().runtime?.patched
                  ? `${i().runtime?.set}${i().runtime?.cache_hit ? ' (cached)' : ''}`
                  : i().runtime
                    ? 'patched image'
                    : 'stock'}
            </span>
          )}
        </Show>
      </label>
      <label
        class="tag opt"
        title={
          dspHere()
            ? `Run the ${processor()} at the next Open or Reload. Off: run the main CPU without it.`
            : `${deviceNameOf(props.device)} has no separate audio processor`
        }
      >
        <input
          type="checkbox"
          checked={bootDsp() && dspHere()}
          disabled={!dspHere()}
          onChange={(e) => chooseDsp(e.currentTarget.checked)}
        />
        <span class="long">{processor()}</span>
        <span class="short">{processor()}</span>
        <Show when={info()?.dsp}>
          {(d) => (
            <span
              class="rt"
              classList={{ on: d().enabled, pending: dspIsPending() }}
              title={
                dspIsPending()
                  ? `${processor()} is ${d().enabled ? 'on' : 'off'} in this session. The setting applies at the next Reload (${props.reloadKey}) or Open.`
                  : `${processor()} is ${d().enabled ? 'on' : 'off'} in this session.`
              }
            >
              {dspIsPending() ? 'on next Reload' : d().enabled ? 'on' : 'off'}
            </span>
          )}
        </Show>
      </label>
      <span class="tb-group">
        <DefaultModules
          device={props.device}
          off={bootRuntime() ? '' : 'Boot with digikit runtime is off: no module can be installed'}
          note="Uploaded and enabled in the emulator after the firmware loads."
        />
      </span>
      <span class="tb-group">
        <button
          type="button"
          class="tb"
          classList={{ pending: runtimePending() }}
          disabled={!source() || props.busy}
          aria-label="Reload"
          title={`Reload the startup file (${props.reloadKey})${runtimePending() ? ' · applies the Runtime choice' : ''}`}
          onClick={() => void reload('Reloaded at')}
        >
          <Icon d={RELOAD} />
          <span class="lbl">Reload</span>
          <kbd class="lbl">{props.reloadKey}</kbd>
        </button>
        <button
          type="button"
          class="tb"
          disabled={!status() || props.busy}
          aria-label="Reload modules"
          title={`Build the stale module objects, then apply the modules into the running session, without a boot when only UI modules changed (${props.modulesKey}). A changed SHARC+ source still needs mise run dk:dev.`}
          onClick={() => void reloadModules()}
        >
          <Icon d={MODULES} />
          <span class="lbl">Modules</span>
          <kbd class="lbl">{props.modulesKey}</kbd>
        </button>
      </span>
      <span class="tb-group">
        <button
          type="button"
          class="tb icon"
          disabled={!props.canPause}
          aria-label={paused() ? 'Resume' : 'Pause'}
          title={paused() ? 'Resume the emulator' : 'Pause the emulator'}
          onClick={() => props.onTogglePause()}
        >
          <Icon d={paused() ? PLAY : PAUSE} />
        </button>
        <button
          type="button"
          class="tb icon"
          disabled={!props.canReset}
          aria-label="Reset (emulator)"
          title="Reset the running firmware to its ready state (the emulator's reset; not the device's RESTART command)"
          onClick={() => props.onReset()}
        >
          <Icon d={RESTART} />
        </button>
        <button
          type="button"
          class="tb icon"
          disabled={!status()}
          aria-label="Save diagnostics"
          title="Save diagnostics"
          onClick={() => props.onSaveDiagnostics()}
        >
          <Icon d={CHART} />
        </button>
      </span>
    </header>
  );
}

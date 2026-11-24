import { Show, type JSX } from 'solid-js';
import type { Job } from '@digi/workbench-core/jobs/jobs-core.ts';
import type { NativeAudioStatus } from '@digi/workbench-core/runtime/runtime.ts';
import { percent, type DeviceHealth } from '../device-health';
import { DOWN, Icon, RELOAD, SPEAKER, UP, WARNING } from '../icons';
import type { DrawerName } from '../log-state';
import { pgoWarning, type BuildInfo } from '../pgo-warning';

function Meter(props: { value: number; worst?: number; tone?: 'teal' | 'amber' }) {
  const pct = (v: number) => `${Math.max(0, Math.min(100, v))}%`;
  return (
    <span class="meter" aria-hidden="true">
      <b class={props.tone ?? 'teal'} style={{ width: pct(props.value) }} />
      <Show when={props.worst !== undefined}>
        <u style={{ left: pct(props.worst!) }} />
      </Show>
    </span>
  );
}

export default function StatusBar(props: {
  isEmulator: boolean;
  inactive: boolean;
  label: string;
  remoteAudio: JSX.Element;
  outputDevice?: NativeAudioStatus['output_device'];
  dspOff: boolean;
  rate?: number;
  buildInfo?: BuildInfo;
  underruns?: number;
  load?: NativeAudioStatus['load'];
  remoteHealth?: Pick<DeviceHealth, 'cpu' | 'dsp'>;
  currentJob?: Job;
  jobCount: number;
  now: number;
  reloadNote: string;
  targetName: string;
  activeDrawer?: DrawerName;
  hasAnalyzer: boolean;
  logKey: string;
  unreadErrors: number;
  onToggleDrawer(name: DrawerName): void;
  onShowMetrics(): void;
}) {
  return (
    <footer class="statusbar">
      <div class="status-readings">
        <span>
          <i
            class="dot"
            classList={{
              paused: props.inactive,
            }}
          />
          {props.label}
        </span>
        {props.remoteAudio}
        <Show when={props.isEmulator && props.outputDevice}>
          <span>
            <Icon d={SPEAKER} />
            {props.outputDevice}
          </span>
        </Show>
        <Show when={props.dspOff}>
          <span title="Emulated audio speed is available when the DSP is running.">DSP off</span>
        </Show>
        <Show when={props.isEmulator && props.rate !== undefined}>
          <span title="Emulated audio seconds per wall second">
            {props.rate!.toFixed(2)}× RT
            <Meter value={props.rate! * 100} tone="amber" />
          </span>
        </Show>
        <Show when={props.isEmulator && pgoWarning(props.buildInfo)}>
          {(w) => (
            <span class="warn pgo" title={w().title}>
              <Icon d={WARNING} />
              {w().label}
            </span>
          )}
        </Show>
        <Show when={props.isEmulator && props.underruns !== undefined}>
          <span classList={{ warn: (props.underruns ?? 0) > 0 }}>underruns {props.underruns}</span>
        </Show>
        <Show when={props.isEmulator && props.load}>
          <span title="Estimated load of the real chips, from the manuals' cycle tables">
            CF {Math.round(props.load!.cf.real_pct)}%
            <Meter value={props.load!.cf.real_pct} />
          </span>
          <Show when={props.load!.dsp}>
            <span title="Estimated DSP load of the real chip. Model figure (as the HEALTH page shows in the emulator) and window peak are in the Metrics tab">
              DSP est. {Math.round(props.load!.dsp!.real_pct)}%
              <Meter value={props.load!.dsp!.real_pct} worst={props.load!.dsp!.real_high_pct} />
            </span>
          </Show>
        </Show>
        <Show when={!props.isEmulator}>
          <span>CF {percent(props.remoteHealth?.cpu)}</span>
          <span>DSP {percent(props.remoteHealth?.dsp)}</span>
        </Show>
        <span class="sp" />
        <Show
          when={props.currentJob}
          fallback={
            <Show when={props.isEmulator && props.reloadNote}>
              <span class="muted" title={props.reloadNote}>
                {props.reloadNote}
              </span>
            </Show>
          }
        >
          {(job) => (
            <span class="job" title={job().detail ? `${job().label}: ${job().detail}` : job().label}>
              <Icon d={RELOAD} />
              <span class="label">
                {job().group === 'emulator' ? 'Emulator' : props.targetName} · {job().label}
              </span>
              <Show when={job().detail}>
                <span class="detail">{job().detail}</span>
              </Show>
              <span>{Math.max(0, Math.round((props.now - job().started) / 1000))} s</span>
              <Show when={job().progress !== undefined}>
                <Meter value={job().progress! * 100} tone="amber" />
                <span>~{Math.round(job().progress! * 100)}%</span>
              </Show>
              <Show when={props.jobCount > 1}>
                <span class="more">+{props.jobCount - 1}</span>
              </Show>
              <Show when={job().cancel}>
                <button type="button" class="cancel" onClick={() => job().cancel?.()}>
                  Cancel
                </button>
              </Show>
            </span>
          )}
        </Show>
      </div>
      <Show when={props.hasAnalyzer}>
        <button
          type="button"
          class="sb"
          aria-expanded={props.activeDrawer === 'Analyzer'}
          title="Show the selected target’s audio analyzer"
          onClick={() => props.onToggleDrawer('Analyzer')}
        >
          Analyzer <Icon d={props.activeDrawer === 'Analyzer' ? DOWN : UP} />
        </button>
      </Show>
      <button
        type="button"
        class="sb"
        aria-expanded={props.activeDrawer === 'Performance'}
        title="Show CF and DSP utilization history"
        onClick={() => props.onToggleDrawer('Performance')}
      >
        Performance <Icon d={props.activeDrawer === 'Performance' ? DOWN : UP} />
      </button>
      <button
        type="button"
        class="sb"
        aria-expanded={props.activeDrawer === 'Log'}
        title={`Log (${props.logKey})`}
        onClick={() => props.onToggleDrawer('Log')}
      >
        Log
        <Show when={props.unreadErrors > 0}>
          <b class="badge" aria-label={`${props.unreadErrors} new error${props.unreadErrors === 1 ? '' : 's'}`}>
            {props.unreadErrors}!
          </b>
        </Show>
        <Icon d={props.activeDrawer === 'Log' ? DOWN : UP} />
      </button>
      <button
        type="button"
        class="sb"
        title="Show Metrics for the selected target"
        onClick={() => props.onShowMetrics()}
      >
        Metrics
      </button>
    </footer>
  );
}

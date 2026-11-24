// The Profile pane: where the ColdFire time goes, per module, per runtime phase and per poll, and on the emulator the DSP per block. A tab of the
// right-hand column (SidePanels.tsx) of the Emulator page and of the Remote page, in the Card panel's look (card-panel.css) and the Memory panel's
// tokens (profile-panel.css). The pure rules (rows, shares, the graph history, the hot rule) are packages/workbench-core/src/profile/profile-core.ts;
// session.ts owns reads and recording commands for the target binding.
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { For, Show, createMemo, onCleanup, untrack, type JSX } from 'solid-js';
import { panels } from '@digi/workbench-core/data/faceplates.ts';
import type { Host } from '@digi/workbench-core/host/host.ts';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import {
  buildRows,
  dspText,
  dspWidth,
  fmtElapsed,
  fmtMhz,
  fmtShare,
  fmtUs,
  groupRows,
  totalShare,
  type DspView,
  type OwnerKind,
} from '@digi/workbench-core/profile/profile-core.ts';
import RecordingButton from '../RecordingButton';
import { cardRefresh } from '../card-state';
import { createDkProfLoader } from '../dkprof-env';
import { createEmulatorNames, createDeviceNames } from '../log-names-backend';
import { deviceOfInfoPort } from '@digi/workbench-core/data/device-profiles.ts';
import { createRecorder } from './recorder';
import { createProfileSession } from './session';
import HistoryGraph from './HistoryGraph';
import HotspotTable from './HotspotTable';
import { fmtPct, resolveSavedHotspots } from './hotspots';
import { createProfileReference } from './reference';
import { OpenProfileButton, ReferenceView } from './ReferenceView';
import type { SpanRecording } from '../span-recording';
import {
  calibrationChip,
  estimateDsp,
  fmtBlockShare,
  fmtCycles,
} from '@digi/workbench-core/profile/calibration-active.ts';
import type { CalibrationHub } from '../calibration-hub';
import '../card-panel.css';
import '../profile-panel.css';

const DEBUG_MODULE = 'digikit/debug';

export default function ProfilePanel(props: {
  visible: boolean;
  collectSpans?: boolean;
  liveDsp?: boolean;
  onSpanRecording?(recording?: SpanRecording): void;
  source: TargetBinding;
  host?: Host;
  /** What the Profile and Calibration tabs share (a run in progress, the active calibration). */
  hub: CalibrationHub;
  tabpanel?: { id: string; labelledBy: string };
}): JSX.Element {
  const app = useTransferApp();
  const { noteReply } = app.connection;
  const { checkRuntime, emulatorRuntime } = app.target;
  const source = untrack(() => props.source);
  const host = untrack(() => props.host);
  const onEmulator = source.target.kind === 'emulator';
  const prof = host ? createDkProfLoader(host) : undefined;
  const recorder = createRecorder({
    session: () => source.session(),
    rpc: onEmulator ? host?.rpc : undefined,
    loadLibrary: app.loadLibrary,
    prof,
    names: host && prof ? (onEmulator ? createEmulatorNames : createDeviceNames)(host, prof) : undefined,
  });

  const {
    reading,
    ok,
    dsp,
    error,
    noRuntime,
    busy,
    locked,
    acting,
    recording,
    columns,
    markers,
    nowShare,
    reset,
    exportFile,
    addDebug,
  } = createProfileSession({
    source,
    recorder,
    visible: () => props.visible,
    collectSpans: () => props.collectSpans,
    liveDsp: () => props.liveDsp,
    checkRuntime,
    emulatorRuntime,
    cardRefresh,
    calibrating: () => props.hub.calibrating(),
    noteReply,
    save: host ? ({ text, name }) => host.fs.saveLog(new TextEncoder().encode(text), name, 'profile') : undefined,
    installDebug: (current) =>
      app.jobs.runJob('Install debug module', { group: 'target' }, async () => {
        source.check();
        await app.backend.install([DEBUG_MODULE], () => {});
        if (!current()) return;
        source.check();
        await app.backend.restart(() => {});
      }),
  });
  untrack(() => props.onSpanRecording?.(recording));
  onCleanup(() => props.onSpanRecording?.(undefined));
  const reference = createProfileReference({
    visible: () => props.visible,
    resolve: (profile) => resolveSavedHotspots(profile, host?.symbols, prof),
  });
  const comparison = () => reference.selection()?.profile.shares;
  const referenceShare = (name: string) => {
    const share = comparison()?.get(name);
    return share === undefined ? '–' : fmtShare(share);
  };

  const rows = createMemo(() => {
    const r = ok();
    return r ? buildRows(r.snap, r.modules) : [];
  });
  const groups = createMemo(() => groupRows(rows()));
  /** The device display name and firmware version from a `<device-id>-<version>` port name. */
  const device = () => {
    const port = reading()?.port ?? '';
    const d = deviceOfInfoPort(port);
    const version = port.slice(port.indexOf('-') + 1);
    return d ? `${panels[d].name}${version && version !== port ? ` ${version}` : ''}` : '';
  };
  /** What the reading came from: the emulator (a controller to ask for the instruction profiles) or a device. */
  const emu = () => reading()?.emulator ?? onEmulator;
  const where = () => (emu() ? 'emulator' : 'USB');
  /** The calibration kept for the device of the reading, and what it estimates for the DSP rows (emulator only: a device measures cycles itself). */
  const calibration = () => props.hub.active.get(deviceOfInfoPort(reading()?.port ?? ''));
  const estimate = (v: DspView) => {
    const c = calibration();
    return c && emu() ? estimateDsp(v, c.doc) : undefined;
  };
  const barPx = (share: number) => Math.min(36, Math.max(2, Math.round(share * 100 * 1.6)));
  const kindClass = (k: OwnerKind) => `k-${k}`;
  const dspSegments = (v: DspView) => v.modules.map((m) => ({ ...m, width: dspWidth(v, m.insnPerBlock) }));

  return (
    <aside
      class="card-panel profile-panel"
      id={props.tabpanel?.id}
      role={props.tabpanel ? 'tabpanel' : undefined}
      aria-labelledby={props.tabpanel?.labelledBy}
      aria-label={props.tabpanel ? undefined : 'Profile'}
      hidden={!props.visible}
    >
      <div class="card-body">
        <Show when={device()}>
          <h3 class="device">
            {device()} · {where()}
          </h3>
        </Show>
        <Show when={reading()?.noDebug}>
          <div class="prof-skeleton" aria-hidden="true">
            <div class="graph" />
            <span class="line" style={{ width: '70%' }} />
            <span class="line" style={{ width: '55%' }} />
            <span class="line" style={{ width: '62%' }} />
          </div>
          <div class="prof-nodebug">
            <strong>Profiling needs the debug module</strong>
            <span class="text">
              The debug module counts the runtime's spans on the device. Without it the runtime pays one flag test per
              span and records nothing.
            </span>
            <div class="acts">
              <button type="button" class="act primary" disabled={locked()} onClick={() => void addDebug()}>
                {acting() === 'add' ? 'Adding…' : 'Add debug module'}
              </button>
            </div>
            <span class="hint">Uploads it with Transfer. The device restarts once.</span>
          </div>
        </Show>
        <Show when={ok()}>
          {(r) => (
            <>
              <div class="prof-head">
                <div class="line1">
                  <span class="state-line">
                    <span class="dot" classList={{ rec: r().snap.running }} />
                    {r().snap.running ? 'Recording' : r().snap.elapsedTicks > 0n ? 'Stopped after ' : 'Stopped'}
                    {!r().snap.running && r().snap.elapsedTicks > 0n
                      ? fmtElapsed(r().snap.elapsedTicks, r().snap.hz)
                      : ''}
                    {r().traceSummary ? ` · ${r().traceSummary}` : ''}
                    {' · clock '}
                    {fmtMhz(r().snap.hz)} {emu() ? '(emulated time)' : '(device clock)'}
                  </span>
                  <span class="elapsed">{r().snap.running ? fmtElapsed(r().snap.elapsedTicks, r().snap.hz) : ''}</span>
                </div>
                <div class="acts">
                  <RecordingButton
                    includeDsp={recording.includeDsp()}
                    dspAvailable={recording.dspAvailable()}
                    setIncludeDsp={recording.setIncludeDsp}
                    running={recording.running()}
                    disabled={locked()}
                    onToggle={recording.toggle}
                  />
                  <button type="button" class="act quiet" disabled={locked()} onClick={() => void reset()}>
                    Reset
                  </button>
                  <span class="sp" />
                  <OpenProfileButton open={reference.open} />
                  <button
                    type="button"
                    class="act quiet"
                    disabled={locked() || !props.host || r().snap.elapsedTicks === 0n}
                    onClick={() => void exportFile()}
                  >
                    Export…
                  </button>
                </div>
              </div>

              <HistoryGraph
                columns={columns()}
                markers={markers()}
                emulator={emu()}
                share={emu() ? nowShare() : totalShare(r().snap)}
              />

              <div class="prof-table" classList={{ cmp: !!comparison() }}>
                <div class="row colhead">
                  <span>Owner</span>
                  <span>Share</span>
                  <span class="r">mean µs</span>
                  <span class="r">max µs</span>
                  <Show when={comparison()}>
                    <span class="r" title={reference.selection()?.name}>
                      file
                    </span>
                  </Show>
                </div>
                <Show when={groups().length === 0}>
                  <p class="empty">
                    {r().snap.running ? 'Waiting for the first spans…' : 'Nothing recorded. Press Start recording.'}
                  </p>
                </Show>
                <For each={groups()}>
                  {(g) => (
                    <>
                      <div class="group">{g.label}</div>
                      <For each={g.rows}>
                        {(row) => (
                          <div class="row">
                            <span class="owner">
                              <span class={`swatch ${kindClass(row.kind)}`} />
                              <span class="name" title={row.name}>
                                {row.name}
                              </span>
                            </span>
                            <span class="share">
                              <span class={`bar ${kindClass(row.kind)}`} style={{ width: `${barPx(row.share)}px` }} />
                              <span class="pct">{fmtShare(row.share)}</span>
                            </span>
                            <span class="r mono">{fmtUs(row.meanUs)}</span>
                            <span
                              class="r mono"
                              classList={{ hot: row.hot }}
                              title={row.hot ? 'The longest call is far above the mean' : undefined}
                            >
                              {fmtUs(row.maxUs)}
                            </span>
                            <Show when={comparison()}>
                              <span class="r mono dim">{referenceShare(row.name)}</span>
                            </Show>
                          </div>
                        )}
                      </For>
                    </>
                  )}
                </For>
              </div>

              <Show when={emu()}>
                <div class="prof-sec ruled">
                  <div class="sec-head">
                    <span class="title">{r().noDsp ? 'ColdFire profile (emulator)' : 'DSP per block (emulator)'}</span>
                    <Show when={dsp()}>{(v) => <span class="num">{dspText(v())}</span>}</Show>
                  </div>
                  <Show
                    when={dsp()}
                    fallback={
                      <span class="hint" hidden={r().noDsp && !!r().cf}>
                        {r().note ??
                          (r().noDsp
                            ? 'No DSP on this device.'
                            : r().snap.running
                              ? 'Waiting for the DSP counts…'
                              : 'Start records the DSP counts of the emulator with the spans.')}
                      </span>
                    }
                  >
                    {(v) => (
                      <>
                        <div class="dspbar" role="img" aria-label={`DSP per block: ${dspText(v())}`}>
                          <For each={dspSegments(v())}>
                            {(m) => <div class={kindClass(m.kind)} style={{ width: `${m.width}%` }} title={m.name} />}
                          </For>
                        </div>
                        <span class="hint">
                          Module instructions per DSP block
                          {v().headroom ? ' against the device headroom' : ''}. Stock DSP work is outside the bar.
                        </span>
                        <Show when={estimate(v())}>
                          <span class="hint" title={calibration()!.name}>
                            {calibrationChip(device(), calibration()!.doc)}
                          </span>
                        </Show>
                        <div class="dsp-mods" classList={{ 'cal-est': !!estimate(v()) }}>
                          <Show when={estimate(v())}>
                            <div class="dsp-mod head" role="row">
                              <span class="name">row</span>
                              <span class="mono">insn/blk</span>
                              <span class="mono est">est. cyc/blk</span>
                              <span class="mono est share">% block</span>
                            </div>
                          </Show>
                          <For each={v().modules}>
                            {(m, i) => (
                              <>
                                <div class="dsp-mod">
                                  <span class={`swatch ${kindClass(m.kind)}`} />
                                  <span class="name">{m.name}</span>
                                  <span class="mono">{Math.round(m.insnPerBlock).toLocaleString('en-US')}</span>
                                  <Show when={estimate(v())?.rows[i()]}>
                                    {(row) => (
                                      <>
                                        <span
                                          class="mono est"
                                          title={`budget factor, worst under ${(estimate(v())!.worstUnder * 100).toFixed(1)} %; the fit factor alone gives ${fmtCycles(m.insnPerBlock * calibration()!.doc.dsp!.factor.fit)}`}
                                        >
                                          {fmtCycles(row().cycles)}
                                        </span>
                                        <span class="mono est share">{fmtBlockShare(row().share)}</span>
                                      </>
                                    )}
                                  </Show>
                                </div>
                                <div class="dsp-top">
                                  <For each={m.top}>
                                    {(t) => (
                                      <div>
                                        <span class="name">{t.label}</span>
                                        <span class="mono">{t.pct}%</span>
                                      </div>
                                    )}
                                  </For>
                                </div>
                              </>
                            )}
                          </For>
                        </div>
                      </>
                    )}
                  </Show>
                  <Show when={r().cf}>
                    {(cf) => (
                      <div class="cf-view">
                        <Show when={!r().noDsp}>
                          <div class="sec-head">
                            <span class="title">ColdFire (emulator)</span>
                          </div>
                        </Show>
                        <div class="cf-paths" role="table" aria-label="ColdFire paths">
                          <div class="cf-row head" role="row">
                            <span class="name">path</span>
                            <span class="mono">host time</span>
                            <span class="mono">insns</span>
                          </div>
                          <For each={cf().paths}>
                            {(p) => (
                              <div class="cf-row" role="row">
                                <span class="name">{p.path}</span>
                                <span class="mono">{fmtPct(p.timeShare)}</span>
                                <span class="mono">{fmtPct(p.insnShare)}</span>
                              </div>
                            )}
                          </For>
                        </div>
                        <Show when={cf().pcs.length > 0}>
                          <HotspotTable title="ColdFire PCs" rows={cf().pcs} />
                        </Show>
                      </div>
                    )}
                  </Show>
                  <Show when={(dsp() || r().cf) && r().note}>
                    <span class="hint">{r().note}</span>
                  </Show>
                </div>
              </Show>
              <Show when={!emu()}>
                <div class="prof-sec ruled">
                  <div class="sec-head">
                    <span class="title">DSP, whole block (HEALTH)</span>
                    <Show when={r().deviceDsp}>
                      {(d) => (
                        <span class="num">
                          {d().avgPct.toFixed(1)}% mean · {d().maxPct.toFixed(1)}% max
                        </span>
                      )}
                    </Show>
                  </div>
                  <Show when={r().deviceDsp}>
                    {(d) => (
                      <div class="dspbar" role="img" aria-label={`DSP block load: ${d().avgPct.toFixed(1)}% mean`}>
                        <div class="k-machine" style={{ width: `${Math.min(100, d().avgPct)}%` }} />
                        <div
                          class="peak"
                          style={{ width: `${Math.max(0, Math.min(100, d().maxPct) - Math.min(100, d().avgPct))}%` }}
                        />
                      </div>
                    )}
                  </Show>
                  <span class="hint">
                    {r().deviceDsp
                      ? 'The whole block as the runtime logs it (HEALTH).'
                      : 'No HEALTH event in the ring yet.'}{' '}
                    Per-module DSP needs the DSP stats window.
                  </span>
                </div>
              </Show>
              <span class="foot">
                Reads every second while this tab is open. Export writes dk.profile/1 (
                {emu() ? 'spans, cf.profile, sharc.profile' : 'spans'}).
              </span>
            </>
          )}
        </Show>
        <Show when={!ok()}>
          <div class="acts">
            <OpenProfileButton open={reference.open} />
          </div>
        </Show>
        <ReferenceView name={reference.selection()?.name} pcs={reference.pcs()} error={reference.error()} />
        <Show when={error()}>
          <p class="err" role="alert">
            {error()}
          </p>
        </Show>
        <Show when={noRuntime() && !reading()}>
          <p class="empty">The runtime is not loaded on this target.</p>
        </Show>
        <Show when={busy() && !reading() && !error()}>
          <p class="empty">Reading…</p>
        </Show>
      </div>
    </aside>
  );
}

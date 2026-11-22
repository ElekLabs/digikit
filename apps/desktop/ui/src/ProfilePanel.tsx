import RecordingButton from './RecordingButton';
// The Profile pane: where the ColdFire time goes, per module, per runtime phase and per poll, and on the emulator the DSP per block. A tab of the
// right-hand column (SidePanels.tsx) of the Emulator page and of the Remote page, in the Card panel's look (card-panel.css) and the Memory panel's
// tokens (profile-panel.css). The pure rules (rows, shares, the graph history, the hot rule) are packages/workbench-core/src/profile/profile-core.ts;
// what is read and done is profile-backend.ts (the recorder), over the target's one session (target.ts).
// Reads: once a second, only while the tab is shown and the target records, one at a time (read-gate-core.ts); a tab that is shown while the target
// is stopped reads once. Start and Stop, Reset and Export are the recorder's. A target without the debug module gets one button that adds it.
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { For, Show, createEffect, createMemo, createSignal, onCleanup, untrack, type JSX } from 'solid-js';
import { panels } from '@digi/workbench-core/data/faceplates.ts';
import { isCancelled, isNoRuntime } from '@digi/workbench-core/modules/backend.ts';
import type { Host } from '@digi/workbench-core/host/host.ts';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { targetLabel } from '@digi/workbench-core/transfer/target.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import {
  buildRows,
  createHistory,
  dspText,
  dspWidth,
  fmtElapsed,
  fmtMhz,
  fmtShare,
  fmtUs,
  graphTop,
  groupRows,
  STACK,
  totalShare,
  type DspView,
  type OwnerKind,
} from '@digi/workbench-core/profile/profile-core.ts';
import { cardRefresh } from './card-state';
import { createDkProfLoader } from './dkprof-env';
import { createEmulatorNames, createDeviceNames } from './log-names-backend';
import { deviceOfPort } from './remote-core';
import { createRecorder, type ProfileReading } from './profile-backend';
import {
  hotspotLabel,
  readSavedProfile,
  resolveSavedHotspots,
  type Hotspot,
  type SavedProfile,
} from './profile-hotspots';
import { createSpanIntervals } from './span-intervals';
import type { SpanRecording } from './span-recording';
import { createReadGate } from './read-gate-core';
import {
  calibrationChip,
  estimateDsp,
  fmtBlockShare,
  fmtCycles,
} from '@digi/workbench-core/profile/calibration-active.ts';
import type { CalibrationHub } from './calibration-hub';
import './card-panel.css';
import './profile-panel.css';

const DEBUG_MODULE = 'digikit/debug';
const LEGEND: { kind: OwnerKind; text: string }[] = [
  { kind: 'machine', text: 'machine' },
  { kind: 'effect', text: 'effect' },
  { kind: 'tool', text: 'tool' },
  { kind: 'runtime', text: 'runtime' },
];
const COLUMN_PX = 4;
const GAP_PX = 2;
const GRAPH_PX = 90;

const fmtPct = (share: number) => `${(share * 100).toFixed(share > 0 && share < 0.001 ? 2 : 1)}%`;

function HotspotTable(props: { title: string; rows: Hotspot[] }): JSX.Element {
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
  const { checkRuntime, emulatorRuntime, target } = app.target;
  const [reading, setReading] = createSignal<ProfileReading>();
  const [dsp, setDsp] = createSignal<DspView>();
  const [error, setError] = createSignal('');
  const [includeDsp, setIncludeDsp] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [acting, setActing] = createSignal('');
  const [noRuntime, setNoRuntime] = createSignal(false);
  const [reference, setReference] = createSignal<{ name: string; profile: SavedProfile }>();
  const [referenceError, setReferenceError] = createSignal('');
  const [referencePcs, setReferencePcs] = createSignal<SavedProfile['pcs']>();
  let opening = 0;
  const history = createHistory();
  const spans = createSpanIntervals();
  const [intervals, setIntervals] = createSignal(spans.intervals());
  function clearSpans() {
    spans.clear();
    setIntervals(spans.intervals());
  }
  const [version, setVersion] = createSignal(0); // the history changed

  const onEmulator = () => props.source.target.kind === 'emulator';

  const prof = props.host ? createDkProfLoader(props.host) : undefined;
  const recorder = createRecorder({
    session: async () => props.source.session(),
    get rpc() {
      return onEmulator() ? props.host?.rpc : undefined;
    },
    loadLibrary: app.loadLibrary,
    prof,
    names: props.host && prof ? (onEmulator() ? createEmulatorNames : createDeviceNames)(props.host, prof) : undefined,
  });

  // a calibration run (the Calibration tab) owns the counters and the target: this pane does not read while it plays and locks its other actions
  const calibrating = props.hub.calibrating;
  const locked = () => !!acting() || calibrating();

  const running = () => {
    const r = reading();
    return !!r && !r.noDebug && r.snap.running;
  };

  const gate = createReadGate({
    run: async (current) => {
      setBusy(true);

      try {
        checkRuntime(props.source.target);
        const r = await recorder.read(props.visible ? 'full' : 'spans', props.liveDsp);
        if (current()) {
          if (!r.noDebug) {
            const previous = reading();
            if (previous && !previous.noDebug && previous.imageId !== r.imageId) clearSpans();
            spans.push(r.observedAt, r.snap, r.modules);
            setIntervals(spans.intervals());
            history.push(r.snap, r.modules);
            setVersion((v) => v + 1);
            if (r.dsp) setDsp(r.dsp);
          }
          if (r.noDebug) spans.break();
          setReading(r);
          setError('');
          setNoRuntime(false);
          void noteReply(await props.source.session(), () => props.source.target === target());
        }
        return true;
      } catch (e) {
        if (!current()) return true;
        spans.break();
        if (isCancelled(e)) return true;
        if (isNoRuntime(e)) {
          setError(failureLine(e, targetLabel(props.source.target)));
          setNoRuntime(true);
          return true;
        }
        setError(failureLine(e, targetLabel(props.source.target)));
        return false;
      } finally {
        setBusy(false);
      }
    },
  });

  const detail = createMemo(() =>
    calibrating() ? undefined : props.visible ? 'full' : props.collectSpans ? 'spans' : undefined,
  );
  createEffect(() => {
    if (props.source.target.kind === 'emulator') emulatorRuntime();
    untrack(clearSpans);
  });
  // read when the tab is shown, when the target changed, when a Transfer job ended or the emulator restarted
  createEffect(() => {
    cardRefresh();

    if (props.source.target.kind === 'emulator') emulatorRuntime();
    untrack(() => {
      history.clear();
      setReading(undefined);
      setDsp(undefined);
      setVersion((v) => v + 1);
    });
  });
  createEffect(() => {
    cardRefresh();

    if (props.source.target.kind === 'emulator') emulatorRuntime();
    if (detail() && !acting()) untrack(() => gate.request());
    else {
      gate.stop();
      if (!detail()) spans.break();
    }
  });
  // while the target records and the tab is shown: once a second
  createEffect(() => {
    if (!detail() || acting() || !running()) return;
    const id = setInterval(() => gate.request(), 1000);
    onCleanup(() => clearInterval(id));
  });
  onCleanup(() => gate.dispose());

  let pending: Promise<void> | undefined;
  let releasing: Promise<void> | undefined;
  const releaseDsp = () =>
    (releasing ??= (async () => {
      await pending?.catch(() => {});
      await recorder.releaseDsp();
    })());
  const unbindDsp = props.source.beforeLeave(releaseDsp);
  onCleanup(() => {
    unbindDsp();
    void releaseDsp().catch(() => {});
  });

  const act = async (what: string, f: () => Promise<void>) => {
    if (locked()) return;
    gate.stop();
    setActing(what);
    try {
      pending = f();
      await pending;
      setError('');
    } catch (e) {
      setError(failureLine(e, targetLabel(props.source.target)));
    } finally {
      pending = undefined;
      setActing('');
    }
  };
  const start = (withDspBlocks: boolean) =>
    act('start', async () => {
      clearSpans();
      history.clear();
      setDsp(undefined);
      await recorder.start(withDspBlocks);
    });
  const stop = () => act('stop', () => recorder.stop());
  const reset = () =>
    act('reset', async () => {
      clearSpans();
      history.clear();
      setDsp(undefined);
      await recorder.reset();
    });
  const exportFile = () =>
    act('export', async () => {
      if (!props.host) return;
      const f = await recorder.exportFile();
      await props.host.fs.saveLog(new TextEncoder().encode(f.text), f.name, 'profile');
    });
  const addDebug = () =>
    act('add', () =>
      app.jobs.runJob('Install debug module', { group: 'target' }, async () => {
        // the debug module is in the library; the runtime loads it at the next start
        props.source.check();
        await app.backend.install([DEBUG_MODULE], () => {});
        props.source.check();
        await app.backend.restart(() => {});
      }),
    );
  const dspAvailable = () => !!ok() && !ok()?.noDsp;
  const toggleRecording = () => void (running() ? stop() : start(dspAvailable() && includeDsp()));
  props.onSpanRecording?.({
    dspBlocks: () => ok()?.dspBlocks,
    dspBlockStatus: () => {
      if (error()) return error();
      const r = ok();
      if (!r) return reading()?.noDebug ? 'Profiling needs the debug module.' : 'Reading target capabilities…';
      return (
        r.dspBlockNote ||
        (r.snap.running
          ? 'Waiting for the first complete DSP sweep…'
          : 'Select Include DSP block costs and start recording.')
      );
    },
    frozenTrace: () => ok()?.frozenTrace,
    timelineStatus: () => {
      const r = ok();
      if (error()) return error();
      if (!r) return reading()?.noDebug ? 'Profiling needs the debug module.' : 'Reading target capabilities…';
      if (!r.traceSupported)
        return 'Individual events require digikit runtime 0.11.3 or later and the updated debug module.';
      if (r.snap.running) return 'Stop recording to view the captured events.';
      return r.traceSummary || 'Start a recording to capture ColdFire events.';
    },
    intervals,
    running,
    available: () => !!reading() && !reading()?.noDebug && !noRuntime(),
    disabled: () => locked() || !!error() || (busy() && !reading()),
    status: () =>
      calibrating()
        ? 'Calibration in progress'
        : error() ||
          (reading()?.noDebug
            ? 'Profiling needs the debug module'
            : !reading()
              ? 'Reading profile state…'
              : running()
                ? 'Recording'
                : ok()?.traceSummary || 'Stopped'),
    includeDsp,
    dspAvailable,
    setIncludeDsp,
    toggle: toggleRecording,
  });
  onCleanup(() => props.onSpanRecording?.(undefined));
  const openReference = async (file: File | undefined) => {
    if (!file) return;
    const request = ++opening;
    try {
      const profile = readSavedProfile(JSON.parse(await file.text()));
      if (request !== opening) return;
      if (!profile.shares && !profile.pcs.cf.length && !profile.pcs.sharc.length) {
        setReferenceError(`${file.name} has no span window or sampled PCs.`);
        return;
      }
      setReference({ name: file.name, profile });
      setReferenceError('');
    } catch (e) {
      if (request === opening) setReferenceError(`${file.name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  createEffect(() => {
    const ref = reference();
    setReferencePcs(ref?.profile.pcs);
    if (!ref || !props.visible) return;
    let current = true;
    onCleanup(() => (current = false));
    void resolveSavedHotspots(ref.profile, props.host?.symbols, prof).then((pcs) => {
      if (current) setReferencePcs(pcs);
    });
  });
  const openProfileButton = () => (
    <label class="act quiet file">
      Open profile…
      <input
        type="file"
        accept=".json,application/json"
        onChange={(e) => {
          void openReference(e.currentTarget.files?.[0]);
          e.currentTarget.value = '';
        }}
      />
    </label>
  );

  const referenceShare = (name: string) => {
    const share = reference()?.profile.shares?.get(name);
    return share === undefined ? '–' : fmtShare(share);
  };

  const ok = () => {
    const r = reading();
    return r && !r.noDebug ? r : undefined;
  };
  const rows = createMemo(() => {
    const r = ok();
    return r ? buildRows(r.snap, r.modules) : [];
  });
  const groups = createMemo(() => groupRows(rows()));
  const cols = createMemo(() => (version(), history.columns()));
  const top = createMemo(() => graphTop(cols()));
  const marks = createMemo(() => (version(), history.markers()).slice(-3));
  const nowShare = createMemo(() => (version(), history.now()));
  /** `Digitone II 1.12`: the device and the firmware version the port name carries (`dn2-1.12`). */
  const device = () => {
    const port = reading()?.port ?? '';
    const d = deviceOfPort(port);
    const version = port.slice(port.indexOf('-') + 1);
    return d ? `${panels[d].name}${version && version !== port ? ` ${version}` : ''}` : '';
  };
  /** What the reading came from: the emulator (a controller to ask for the instruction profiles) or a device. */
  const emu = () => reading()?.emulator ?? onEmulator();
  const where = () => (emu() ? 'emulator' : 'USB');
  /** The calibration kept for the device of the reading, and what it estimates for the DSP rows (emulator only: a device measures cycles itself). */
  const calibration = () => props.hub.active.get(deviceOfPort(reading()?.port ?? ''));
  const estimate = (v: DspView) => {
    const c = calibration();
    return c && emu() ? estimateDsp(v, c.doc) : undefined;
  };
  const px = (share: number) => Math.round((share / top()) * GRAPH_PX);
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
                    includeDsp={includeDsp()}
                    dspAvailable={dspAvailable()}
                    setIncludeDsp={setIncludeDsp}
                    running={running()}
                    disabled={locked()}
                    onToggle={toggleRecording}
                  />
                  <button type="button" class="act quiet" disabled={locked()} onClick={() => void reset()}>
                    Reset
                  </button>
                  <span class="sp" />
                  {openProfileButton()}
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

              <div class="prof-sec">
                <div class="sec-head">
                  <span class="title">{emu() ? 'ColdFire, last 60 s' : 'ColdFire, recorded window'}</span>
                  <span class="num">
                    {emu() ? `${fmtShare(nowShare())} now` : `${fmtShare(totalShare(r().snap))} mean`}
                  </span>
                </div>
                <div class="graph" role="img" aria-label="ColdFire share per owner type over time">
                  <For each={cols()}>
                    {(c) => (
                      <div class="col">
                        <For each={STACK}>
                          {(k) => <div class={kindClass(k)} style={{ height: `${px(c[k])}px` }} />}
                        </For>
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
                        <span class={`swatch ${kindClass(l.kind)}`} />
                        {l.text}
                      </li>
                    )}
                  </For>
                </ul>
              </div>

              <div class="prof-table" classList={{ cmp: !!reference()?.profile.shares }}>
                <div class="row colhead">
                  <span>Owner</span>
                  <span>Share</span>
                  <span class="r">mean µs</span>
                  <span class="r">max µs</span>
                  <Show when={reference()?.profile.shares}>
                    <span class="r" title={reference()?.name}>
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
                            <Show when={reference()?.profile.shares}>
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
          <div class="acts">{openProfileButton()}</div>
        </Show>
        <Show when={referencePcs()} keyed>
          {(pcs) => (
            <Show when={pcs.cf.length || pcs.sharc.length}>
              <div class="prof-sec ruled">
                <div class="sec-head">
                  <span class="title" title={reference()?.name}>
                    Saved hotspots · {reference()?.name}
                  </span>
                </div>
                <Show when={pcs.cf.length}>
                  <HotspotTable title="ColdFire PCs" rows={pcs.cf} />
                </Show>
                <Show when={pcs.sharc.length}>
                  <HotspotTable title="SHARC PCs" rows={pcs.sharc} />
                </Show>
              </div>
            </Show>
          )}
        </Show>
        <Show when={referenceError()}>
          <p class="err" role="alert">
            {referenceError()}
          </p>
        </Show>
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

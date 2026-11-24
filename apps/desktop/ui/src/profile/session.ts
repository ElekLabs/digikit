import { batch, createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js';
import { isCancelled, isNoRuntime } from '@digi/workbench-core/modules/backend.ts';
import { createHistory, type DspView } from '@digi/workbench-core/profile/profile-core.ts';
import { targetLabel, type TargetApp, type TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import type { ConnectionApp } from '@digi/workbench-core/transfer/connection-state.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import { createReadGate } from '../read-gate-core';
import { createSpanIntervals } from '../span-intervals';
import type { SpanRecording } from '../span-recording';
import type { ProfileReading, Recorder } from './recorder';

interface ProfileSessionDeps {
  source: TargetBinding;
  recorder: Recorder;
  visible(): boolean;
  collectSpans(): boolean | undefined;
  liveDsp(): boolean | undefined;
  checkRuntime: TargetApp['checkRuntime'];
  emulatorRuntime: TargetApp['emulatorRuntime'];
  cardRefresh(): unknown;
  calibrating(): boolean;
  noteReply: ConnectionApp['noteReply'];
  save?(file: { text: string; name: string }): Promise<unknown>;
  installDebug(current: () => boolean): Promise<void>;
}

type Action = 'start' | 'stop' | 'reset' | 'export' | 'add';

/** Polls and controls the recorder under the Solid owner of one target binding. */
export function createProfileSession(deps: ProfileSessionDeps) {
  const { recorder, source, calibrating, save } = deps;
  const onEmulator = source.target.kind === 'emulator';
  const label = targetLabel(source.target);
  const [reading, setReading] = createSignal<ProfileReading>();
  const [dsp, setDsp] = createSignal<DspView>();
  const [error, setError] = createSignal('');
  const [includeDsp, setIncludeDsp] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [acting, setActing] = createSignal<Action>();
  const [noRuntime, setNoRuntime] = createSignal(false);
  const [leaving, setLeaving] = createSignal(false);
  const history = createHistory();
  const [historyVersion, setHistoryVersion] = createSignal(0);
  const spans = createSpanIntervals();
  const [intervals, setIntervals] = createSignal(spans.intervals());
  const clearSpans = () => {
    spans.clear();
    setIntervals(spans.intervals());
  };
  const ok = () => {
    const r = reading();
    return r && !r.noDebug ? r : undefined;
  };
  const running = () => ok()?.snap.running ?? false;
  const locked = () => leaving() || !!acting() || calibrating();
  const detail = createMemo(() =>
    leaving() || calibrating() ? undefined : deps.visible() ? 'full' : deps.collectSpans() ? 'spans' : undefined,
  );

  const gate = createReadGate({
    async run(wanted) {
      const readDetail = detail();
      if (!readDetail || locked()) return true;
      const current = () => wanted() && !leaving() && !calibrating();
      setBusy(true);
      try {
        deps.checkRuntime(source.target);
        const next = await recorder.read(readDetail, deps.liveDsp());
        if (!current()) return true;
        batch(() => {
          if (next.noDebug) spans.break();
          else {
            const previous = ok();
            if (previous && previous.imageId !== next.imageId) clearSpans();
            spans.push(next.observedAt, next.snap, next.modules);
            setIntervals(spans.intervals());
            history.push(next.snap, next.modules);
            setHistoryVersion((v) => v + 1);
            if (next.dsp) setDsp(next.dsp);
          }
          setReading(next);
          setError('');
          setNoRuntime(false);
        });
        if (!current()) return true;
        const session = await source.session();
        if (current()) void deps.noteReply(session, current);
        return true;
      } catch (e) {
        if (!current()) return true;
        spans.break();
        if (isCancelled(e)) return true;
        setError(failureLine(e, label));
        if (!isNoRuntime(e)) return false;
        setNoRuntime(true);
        return true;
      } finally {
        if (!leaving()) setBusy(false);
      }
    },
  });

  createEffect(() => {
    if (leaving()) return;
    if (onEmulator) deps.emulatorRuntime();
    untrack(clearSpans);
  });
  createEffect(() => {
    if (leaving()) return;
    deps.cardRefresh();
    if (onEmulator) deps.emulatorRuntime();
    untrack(() => {
      history.clear();
      setReading(undefined);
      setDsp(undefined);
      setHistoryVersion((v) => v + 1);
    });
  });
  createEffect(() => {
    deps.cardRefresh();
    if (onEmulator) deps.emulatorRuntime();
    if (detail() && !acting()) untrack(gate.request);
    else {
      gate.stop();
      if (!detail()) spans.break();
    }
  });
  createEffect(() => {
    if (!detail() || acting() || !running()) return;
    const timer = setInterval(gate.request, 1000);
    onCleanup(() => clearInterval(timer));
  });

  let pending: Promise<void> | undefined;
  let releasing: Promise<void> | undefined;
  const leave = () => {
    if (!releasing) {
      releasing = (async () => {
        await pending?.catch(() => {});
        await recorder.releaseDsp();
      })();
      gate.dispose();
      setLeaving(true);
    }
    return releasing;
  };
  const unbind = source.beforeLeave(leave);
  onCleanup(() => {
    unbind();
    void leave().catch(() => {});
  });

  const act = async (action: Action, work: () => Promise<void>) => {
    if (locked()) return;
    gate.stop();
    pending = Promise.resolve().then(async () => {
      if (!leaving() && !calibrating()) await work();
    });
    setActing(action);
    try {
      await pending;
      if (!leaving()) setError('');
    } catch (e) {
      if (!leaving()) setError(failureLine(e, label));
    } finally {
      pending = undefined;
      if (!leaving()) setActing(undefined);
    }
  };
  const clearRecording = () => {
    clearSpans();
    history.clear();
    setDsp(undefined);
  };
  const exportFile = () =>
    act('export', async () => {
      if (!save) return;
      const file = await recorder.exportFile();
      await save(file);
    });
  const dspAvailable = () => !!ok() && !ok()?.noDsp;
  const recording: SpanRecording = {
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
    available: () => !!ok() && !noRuntime(),
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
    setIncludeDsp: (value) => {
      if (!locked() && !running()) setIncludeDsp(value);
    },
    toggle: () => {
      if (running()) void act('stop', () => recorder.stop());
      else
        void act('start', async () => {
          clearRecording();
          await recorder.start(untrack(() => dspAvailable() && includeDsp()));
        });
    },
  };

  return {
    reading,
    ok,
    dsp,
    busy,
    noRuntime,
    error,
    locked,
    acting,
    recording,
    columns: () => (historyVersion(), history.columns()),
    markers: () => (historyVersion(), history.markers()),
    nowShare: () => (historyVersion(), history.now()),
    reset: () =>
      act('reset', async () => {
        clearRecording();
        await recorder.reset();
      }),
    exportFile,
    addDebug: () => act('add', () => deps.installDebug(() => !untrack(leaving))),
  };
}

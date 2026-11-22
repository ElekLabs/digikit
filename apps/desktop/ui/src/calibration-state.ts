// The state and handlers of the Calibration section, owned by the Profile pane (ProfilePanel.tsx), not by the section: a run that installs modules and
// restarts the target resets the pane's reading and unmounts what is drawn from it, and the run must go on with its captures. No module state: the
// pane builds one with its calibrator and disposes it with the pane (`dispose` aborts a run in progress). Host tests: tests/calibration-state.test.ts.
import { createMemo, createResource, createSignal } from 'solid-js';
import {
  fitView,
  trackNote,
  phaseLine,
  progressLine,
  captureCounter,
  captureTotal,
  readCaptureSet,
  remainingSeconds,
  tableRows,
  type CalibrationDoc,
  type CaptureSet,
  type CalProfileDoc,
  type Side,
} from '@digi/workbench-core/profile/calibration-view.ts';
import type { Capture } from '@digi/workbench-core/calibration/runner.ts';
import type { Calibrator } from './calibration-backend';

const asText = (e: unknown) => (e instanceof Error ? e.message : String(e));
const other = (s: Side): Side => (s === 'emu' ? 'dev' : 'emu');

export interface CalibrationStateDeps {
  calibrator: Calibrator;
  /** The pane's target is the emulator (a run fills the emulator column). */
  emulator: () => boolean;
  /** Something else of the pane is under way (a read in flight, another action): Run refuses. */
  blocked: () => boolean;
  visible?: () => boolean;
  /** The run starts or ends (the pane stops reading while it runs and locks its other actions). */
  onRunning?: (running: boolean) => void;
}

export function createCalibrationState(deps: CalibrationStateDeps) {
  const { calibrator } = deps;
  const choices = createMemo(() => {
    try {
      return { list: calibrator.profiles(), error: '' };
    } catch (e) {
      return { list: [], error: asText(e) };
    }
  });
  const [chosen, setChosen] = createSignal('');
  const profile = () => choices().list.find((p) => p.name === chosen()) ?? choices().list[0];
  // the tracks the chosen profile uses on the target, for the note before Run (a target that cannot say shows no note)
  const [tracks] = createResource(
    () => (deps.visible?.() ?? true) && profile()?.name,
    async (name) => (await calibrator.tracks?.(name).catch(() => [] as number[])) ?? [],
  );
  const note = () => trackNote(tracks() ?? []);
  const [running, setRunning] = createSignal(false);
  const [stopping, setStopping] = createSignal(false);
  const [progress, setProgress] = createSignal('');
  const [notes, setNotes] = createSignal<string[]>([]);
  const [error, setError] = createSignal('');
  const [sets, setSets] = createSignal<{ emu?: CaptureSet; dev?: CaptureSet }>({});
  const [fit, setFit] = createSignal<CalibrationDoc>();
  let stop: AbortController | undefined;

  const rows = createMemo(() =>
    tableRows(
      {
        emu: sets().emu?.captures as CalProfileDoc[] | undefined,
        dev: sets().dev?.captures as CalProfileDoc[] | undefined,
      },
      fit(),
    ),
  );
  const view = createMemo(() => {
    const f = fit();
    return f ? fitView(f) : undefined;
  });
  const both = () => !!sets().emu && !!sets().dev;

  const setRun = (v: boolean) => {
    setRunning(v);
    deps.onRunning?.(v);
  };

  async function run(): Promise<void> {
    const choice = profile();
    if (!choice || running() || deps.blocked()) return;
    setError('');
    setNotes([]);
    setStopping(false);
    const mine = new AbortController();
    stop = mine;
    const side: Side = deps.emulator() ? 'emu' : 'dev';
    setRun(true);
    setProgress('starting…');
    // what the line shows: a capture of a case, or a phase between captures; `done` counts the finished captures
    let shown:
      { kind: 'capture'; case: string } | { kind: 'phase'; event: Parameters<typeof phaseLine>[1] } | undefined;
    let done = 0;
    let skipping: string[] = [];
    // the captures finished per case, and the cases a failure ended early (with the captures they finished)
    const completed = new Map<string, number>();
    const dropped = new Map<string, number>();
    let current: string | undefined;
    const live: Capture['profile'][] = [];
    // the line follows the run's position
    const refresh = () => {
      const s = shown;
      if (!s || !running()) return;
      const total = captureTotal(choice.profile, skipping, dropped);
      const counter = captureCounter({ done, total, capturing: s.kind === 'capture' });
      setProgress(
        s.kind === 'phase'
          ? phaseLine(counter, s.event)
          : progressLine({
              counter,
              case: s.case,
              left: remainingSeconds({
                profile: choice.profile,
                skipping,
                caseId: current,
                n: completed.get(s.case) ?? 0,
                capturing: true,
                dropped,
              }),
            }),
      );
    };
    try {
      const out = await calibrator.run({
        profile: choice.name,
        signal: mine.signal,
        onCapture: (c) => {
          done++;
          completed.set(c.caseId, (completed.get(c.caseId) ?? 0) + 1);
          live.push(c.profile);
          // the table fills as the captures arrive
          setSets((s) => ({
            ...s,
            [side]: { side, profile: choice.name, source: 'this run', captures: [...live] },
          }));
          setFit(undefined);
        },
        onEvent: (e) => {
          if (e.kind === 'plan') {
            skipping = e.skipping;
          } else if (e.kind === 'restart' || e.kind === 'stable' || e.kind === 'restore') {
            shown = { kind: 'phase', event: e };
            refresh();
          } else if (e.kind === 'capture') {
            shown = { kind: 'capture', case: e.case };
            current = e.case;
            refresh();
          } else if (e.kind === 'skip') {
            skipping = [...skipping, e.case];
            setNotes((n) => [...n, `case ${e.case} skipped: ${e.reason}`]);
          } else if (e.kind === 'fail') {
            dropped.set(e.case, completed.get(e.case) ?? 0);
            setNotes((n) => [...n, `case ${e.case} failed: ${e.reason}`]);
            refresh();
          }
        },
      });
      if (out.captures.length > 0) {
        setSets((s) => ({
          ...s,
          [side]: { side, profile: out.profile, source: 'this run', captures: out.captures.map((c) => c.profile) },
        }));
        setFit(undefined);
      }
      if (out.error) setError(`${out.error}${out.captures.length ? ` (${out.captures.length} captures kept)` : ''}`);
      else if (out.stopped) setNotes((n) => [...n, `stopped, ${out.captures.length} captures kept`]);
      else if (out.captures.length === 0) setError('nothing was captured: every case was skipped');
    } catch (e) {
      setError(asText(e));
    } finally {
      setRun(false);
      setStopping(false);
      setProgress('');
      if (stop === mine) stop = undefined;
    }
  }

  const stopRun = () => {
    setStopping(true);
    stop?.abort();
  };

  const pick = async (list: FileList | null | undefined): Promise<CaptureSet | undefined> => {
    const files = Array.from(list ?? []);
    if (files.length === 0) return undefined;
    try {
      return readCaptureSet(await Promise.all(files.map(async (f) => ({ name: f.name, text: await f.text() }))));
    } catch (e) {
      setError(asText(e));
      return undefined;
    }
  };
  const doFit = async (s: { emu?: CaptureSet; dev?: CaptureSet }) => {
    if (!s.emu || !s.dev) return;
    try {
      setFit(await calibrator.fit(s.emu, s.dev));
      setError('');
    } catch (e) {
      setFit(undefined);
      setError(asText(e));
    }
  };
  const load = async (list: FileList | null | undefined) => {
    const set = await pick(list);
    if (!set) return;
    setSets((s) => ({ ...s, [set.side]: set }));
    setFit(undefined);
    setError('');
  };
  const fitWith = async (list: FileList | null | undefined) => {
    const set = await pick(list);
    if (!set) return;
    const next = { ...sets(), [set.side]: set };
    if (!next[other(set.side)]) {
      const here = set.side === 'emu' ? 'an emulator' : 'a device';
      setError(
        `${set.source} is ${here} run: run the profile on ${set.side === 'emu' ? 'the device' : 'the emulator'} first, or load a ${set.side === 'emu' ? 'device' : 'emulator'} file.`,
      );
      return;
    }
    setSets(next);
    await doFit(next);
  };

  return {
    choices,
    chosen: () => profile()?.name ?? '',
    setChosen,
    profile,
    running,
    stopping,
    progress,
    note,
    notes,
    error,
    setError,
    sets,
    fit,
    rows,
    view,
    both,
    run,
    stop: stopRun,
    load,
    fitWith,
    fitNow: () => doFit(sets()),
    /** The pane is gone: a run in progress ends. */
    dispose: () => stop?.abort(),
  };
}

export type CalibrationState = ReturnType<typeof createCalibrationState>;

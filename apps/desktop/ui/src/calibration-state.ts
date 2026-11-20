// The state and handlers of the Calibration section, owned by the Profile pane (ProfilePanel.tsx), not by the section: a run that installs modules and
// restarts the target resets the pane's reading and unmounts what is drawn from it, and the run must go on with its captures. No module state: the
// pane builds one with its calibrator and disposes it with the pane (`dispose` aborts a run in progress). Host tests: tests/calibration-state.test.ts.
import { createMemo, createResource, createSignal } from 'solid-js';
import {
  fitView,
  trackNote,
  phaseLine,
  progressLine,
  readCaptureSet,
  secondsLeft,
  tableRows,
  type CalibrationDoc,
  type CaptureSet,
  type CalProfileDoc,
  type Side,
} from '@digi/workbench-core/profile/calibration-view.ts';
import type { Calibrator } from './calibration-backend';

const asText = (e: unknown) => (e instanceof Error ? e.message : String(e));
const other = (s: Side): Side => (s === 'emu' ? 'dev' : 'emu');

export interface CalibrationStateDeps {
  calibrator: Calibrator;
  /** The pane's target is the emulator (a run fills the emulator column). */
  emulator: () => boolean;
  /** Something else of the pane is under way (a read in flight, another action): Run refuses. */
  blocked: () => boolean;
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
    () => profile()?.name,
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
    let at: { case: string; n: number; of: number; since: number } | undefined;
    let skipping: string[] = [];
    let now: () => Promise<number> = async () => Date.now();
    const left = (n: number, kase: string, into: number) =>
      secondsLeft({ profile: choice.profile, case: kase, n, intoCaptureS: into, skipping });
    // the time left follows the target's clock (emulated time on the emulator)
    const tick = setInterval(() => {
      const a = at;
      if (!a) return;
      void now().then((t) =>
        setProgress(progressLine({ ...a, left: left(a.n, a.case, Math.max(0, (t - a.since) / 1000)) })),
      );
    }, 1000);
    try {
      const out = await calibrator.run({
        profile: choice.name,
        signal: mine.signal,
        onClock: (f) => {
          now = f;
        },
        onEvent: (e) => {
          if (e.kind === 'plan') {
            skipping = e.skipping;
          } else if (e.kind === 'restart' || e.kind === 'stable' || e.kind === 'restore') {
            at = undefined;
            setProgress(phaseLine(e));
          } else if (e.kind === 'capture') {
            void now().then((t) => {
              at = { case: e.case, n: e.n, of: e.of, since: t };
              setProgress(progressLine({ ...e, left: left(e.n, e.case, 0) }));
            });
          } else if (e.kind === 'skip') setNotes((n) => [...n, `case ${e.case} skipped: ${e.reason}`]);
          else if (e.kind === 'fail') setNotes((n) => [...n, `case ${e.case} failed: ${e.reason}`]);
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
      clearInterval(tick);
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

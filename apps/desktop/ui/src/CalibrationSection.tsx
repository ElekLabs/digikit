// The Calibration section of the Profile pane: one button plays the chosen calibration profile on the pane's target (the shared runner of
// workbench-core, through calibration-backend.ts), the table shows what it captured per case and DSP row, and a fit pairs an emulator run with a device
// run. The rules (table rows, capture set files, the lines of the fit, progress) are packages/workbench-core/src/profile/calibration-view.ts; this
// file holds signals and handlers. The look is the Profile pane's (profile-panel.css).
import { For, Show, createMemo, createSignal, onCleanup, type JSX } from 'solid-js';
import type { Host } from '@digi/workbench-core/host/host.ts';
import {
  calibrationFileName,
  captureSetFile,
  fitView,
  fmtCount,
  fmtError,
  fmtFactor,
  groupByCase,
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

export default function CalibrationSection(props: {
  calibrator: Calibrator;
  /** The save dialog. */
  host?: Host;
  /** `Emulator Digitone II 1.12`: the target the run plays on. */
  target: string;
  /** The target is the emulator (a run fills the emulator column). */
  emulator: boolean;
  /** Another action of the pane is under way. */
  disabled?: boolean;
  /** The section runs (the pane stops reading while it does: the run owns the counters). */
  onRunning?: (running: boolean) => void;
}): JSX.Element {
  const choices = createMemo(() => {
    try {
      return { list: props.calibrator.profiles(), error: '' };
    } catch (e) {
      return { list: [], error: asText(e) };
    }
  });
  const [chosen, setChosen] = createSignal('');
  const profile = () => choices().list.find((p) => p.name === chosen()) ?? choices().list[0];
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

  const setRunning2 = (v: boolean) => {
    setRunning(v);
    props.onRunning?.(v);
  };

  const run = async () => {
    const choice = profile();
    if (!choice || running()) return;
    setError('');
    setNotes([]);
    setStopping(false);
    stop = new AbortController();
    setRunning2(true);
    setProgress('starting…');
    const side: Side = props.emulator ? 'emu' : 'dev';
    // the clock of the time left: wall time since the capture started
    let at: { case: string; n: number; of: number; since: number } | undefined;
    const tick = setInterval(() => {
      if (!at) return;
      setProgress(
        progressLine({
          case: at.case,
          n: at.n,
          of: at.of,
          left: secondsLeft({
            profile: choice.profile,
            case: at.case,
            n: at.n,
            intoCaptureS: (Date.now() - at.since) / 1000,
          }),
        }),
      );
    }, 1000);
    try {
      const out = await props.calibrator.run({
        profile: choice.name,
        signal: stop.signal,
        onEvent: (e) => {
          if (e.kind === 'capture') {
            at = { case: e.case, n: e.n, of: e.of, since: Date.now() };
            setProgress(
              progressLine({
                ...e,
                case: e.case,
                left: secondsLeft({ profile: choice.profile, case: e.case, n: e.n, intoCaptureS: 0 }),
              }),
            );
          } else if (e.kind === 'skip') setNotes((n) => [...n, `case ${e.case} skipped: ${e.reason}`]);
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
      setRunning2(false);
      setStopping(false);
      setProgress('');
      stop = undefined;
    }
  };
  const stopRun = () => {
    setStopping(true);
    stop?.abort();
  };
  onCleanup(() => stop?.abort());

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
      setFit(await props.calibrator.fit(s.emu, s.dev));
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
      setError(
        `${set.source} is ${set.side === 'emu' ? 'an emulator' : 'a device'} run: run the profile on ${set.side === 'emu' ? 'the device' : 'the emulator'} first, or load a ${set.side === 'emu' ? 'device' : 'emulator'} file.`,
      );
      return;
    }
    setSets(next);
    await doFit(next);
  };
  let loadInput: HTMLInputElement | undefined;
  let fitInput: HTMLInputElement | undefined;

  const saveFile = async (text: string, name: string) => {
    if (!props.host) return;
    try {
      await props.host.fs.saveLog(new TextEncoder().encode(text), name, 'file');
    } catch (e) {
      setError(asText(e));
    }
  };
  const saveCalibration = () => {
    const f = fit();
    if (f) void saveFile(`${JSON.stringify(f, null, 2)}\n`, calibrationFileName(f));
  };
  const saveRun = () => {
    const s = props.emulator ? sets().emu : sets().dev;
    if (!s) return;
    const f = captureSetFile({
      profile: s.profile ?? 'run',
      captures: s.captures.map((profile) => ({ profile: profile as unknown as Record<string, unknown> })),
    });
    void saveFile(f.text, f.name);
  };

  return (
    <div class="prof-sec ruled cal">
      <div class="sec-head">
        <span class="title">Calibration</span>
        <Show when={profile()}>
          <span class="num">about {Math.ceil((profile()!.seconds || 0) / 60)} min</span>
        </Show>
      </div>
      <div class="cal-bar">
        <label class="cal-pick">
          <span>Profile</span>
          <select
            value={profile()?.name ?? ''}
            disabled={running()}
            onChange={(e) => setChosen(e.currentTarget.value)}
            aria-label="Calibration profile"
          >
            <For each={choices().list}>{(p) => <option value={p.name}>{p.name}</option>}</For>
          </select>
        </label>
        <span class="cal-target" title="The run plays on the target of this pane">
          Target: {props.target}
        </span>
        <Show
          when={running()}
          fallback={
            <button
              type="button"
              class="act primary"
              disabled={!profile() || !!props.disabled}
              onClick={() => void run()}
            >
              Run
            </button>
          }
        >
          <button type="button" class="act" disabled={stopping()} onClick={stopRun}>
            {stopping() ? 'Stopping…' : 'Stop'}
          </button>
        </Show>
      </div>
      <Show when={profile()?.description}>
        <span class="hint">{profile()!.description}</span>
      </Show>
      <Show when={running()}>
        <p class="cal-progress" role="status">
          {progress()}
        </p>
      </Show>
      <div class="prof-table cal-table" role="table" aria-label="Calibration captures">
        <div class="row colhead" role="row">
          <span>case / row</span>
          <span class="r">emu insn/blk</span>
          <span class="r">dev cyc/blk</span>
          <span class="r">factor</span>
          <span class="r">error</span>
        </div>
        <Show when={rows().length === 0}>
          <p class="empty">Nothing captured. Run a profile, or load a saved run.</p>
        </Show>
        <For each={groupByCase(rows())}>
          {(g) => (
            <>
              <div class="group">{g.case}</div>
              <For each={g.rows}>
                {(r) => (
                  <div class="row" role="row">
                    <span class="name" title={r.label}>
                      {r.what}
                    </span>
                    <span class="r mono">{fmtCount(r.emu)}</span>
                    <span class="r mono">{fmtCount(r.dev)}</span>
                    <span class="r mono">{fmtFactor(r.factor)}</span>
                    <span class="r mono" classList={{ hot: (r.error ?? 0) < -0.02 }}>
                      {fmtError(r.error)}
                    </span>
                  </div>
                )}
              </For>
            </>
          )}
        </For>
      </div>
      <Show when={sets().emu || sets().dev}>
        <span class="hint">
          emulator: {sets().emu ? `${sets().emu!.captures.length} captures (${sets().emu!.source})` : 'none'} · device:{' '}
          {sets().dev ? `${sets().dev!.captures.length} captures (${sets().dev!.source})` : 'none'}
        </span>
      </Show>
      <Show when={view()}>
        {(v) => (
          <div class="cal-fit">
            <div class="cal-headline mono">{v().headline}</div>
            <For each={v().details}>{(d) => <div class="hint">{d}</div>}</For>
            <For each={v().refused}>{(d) => <div class="cal-refused">refused: {d}</div>}</For>
            <For each={v().warnings}>{(d) => <div class="cal-warning">warning: {d}</div>}</For>
          </div>
        )}
      </Show>
      <For each={notes()}>{(n) => <span class="hint">{n}</span>}</For>
      <Show when={error() || choices().error}>
        <p class="err" role="alert">
          {error() || choices().error}
        </p>
      </Show>
      <div class="acts cal-acts">
        <button type="button" class="act quiet" disabled={running()} onClick={() => loadInput?.click()}>
          Load file…
        </button>
        <button
          type="button"
          class="act quiet"
          disabled={running() || (!sets().emu && !sets().dev)}
          onClick={() => (both() ? void doFit(sets()) : fitInput?.click())}
        >
          {both() ? 'Fit' : 'Fit with…'}
        </button>
        <span class="sp" />
        <button
          type="button"
          class="act quiet"
          disabled={running() || !props.host || !(props.emulator ? sets().emu : sets().dev)}
          onClick={saveRun}
        >
          Save run…
        </button>
        <button
          type="button"
          class="act quiet"
          disabled={running() || !props.host || !view()?.hasFit}
          onClick={saveCalibration}
        >
          Save calibration…
        </button>
        <input
          ref={loadInput}
          type="file"
          multiple
          hidden
          accept=".json,application/json"
          onChange={(e) => {
            void load(e.currentTarget.files);
            e.currentTarget.value = '';
          }}
        />
        <input
          ref={fitInput}
          type="file"
          multiple
          hidden
          accept=".json,application/json"
          onChange={(e) => {
            void fitWith(e.currentTarget.files);
            e.currentTarget.value = '';
          }}
        />
      </div>
      <span class="foot">
        A run plays the profile on the target and captures DSP stats and spans. Fit pairs an emulator run with a device
        run of the same profile (dk.calibration/1).
      </span>
    </div>
  );
}

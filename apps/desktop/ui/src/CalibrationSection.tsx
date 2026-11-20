// The Calibration section of the Profile pane: one button plays the chosen calibration profile on the pane's target (the shared runner of
// workbench-core, through calibration-backend.ts), the table shows what it captured per case and DSP row, and a fit pairs an emulator run with a device
// run. The rules (table rows, capture set files, the lines of the fit, progress) are packages/workbench-core/src/profile/calibration-view.ts; this
// file holds signals and handlers. The look is the Profile pane's (profile-panel.css).
import { For, Show, type JSX } from 'solid-js';
import type { Host } from '@digi/workbench-core/host/host.ts';
import {
  calibrationFileName,
  captureSetFile,
  fmtCount,
  fmtError,
  fmtFactor,
  groupByCase,
} from '@digi/workbench-core/profile/calibration-view.ts';
import type { CalibrationState } from './calibration-state';

const asText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function CalibrationSection(props: {
  /** The state the pane owns (calibration-state.ts): it outlives this section. */
  state: CalibrationState;
  /** The save dialog. */
  host?: Host;
  /** `Emulator <device name> <firmware version>`: the target the run plays on. */
  target: string;
  /** The target is the emulator (a run fills the emulator column). */
  emulator: boolean;
  /** Another action of the pane is under way. */
  disabled?: boolean;
}): JSX.Element {
  const st = props.state;
  const { running, stopping, sets, rows, view, choices } = st;
  const profile = st.profile;
  let loadInput: HTMLInputElement | undefined;
  let fitInput: HTMLInputElement | undefined;
  const saveFile = async (text: string, name: string) => {
    if (!props.host) return;
    try {
      await props.host.fs.saveLog(new TextEncoder().encode(text), name, 'file');
    } catch (e) {
      st.setError(asText(e));
    }
  };
  const saveCalibration = () => {
    const f = st.fit();
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
  const error = st.error;
  const notes = st.notes;
  const note = st.note;
  const progress = st.progress;
  const both = st.both;
  const stopRun = st.stop;
  const run = st.run;
  const setChosen = st.setChosen;
  const load = st.load;
  const fitWith = st.fitWith;
  const doFit = st.fitNow;

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
      <Show when={note()}>
        <span class="hint">{note()}</span>
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
          onClick={() => (both() ? void doFit() : fitInput?.click())}
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

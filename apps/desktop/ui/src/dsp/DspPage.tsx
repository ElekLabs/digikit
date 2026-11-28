import { For, Show, batch, createEffect, createMemo, createResource, createSignal, on, onCleanup } from 'solid-js';
import type { FsHost, Host } from '@digi/workbench-core/host/host.ts';
import type { DspReportSource, SavedSession } from '@digi/workbench-core/host/session.ts';
import { readQualityReport, type QualityReport } from './quality-report';
import ComparisonView from './ComparisonView';
import './dsp-page.css';

const valueText = (value: unknown) => (value === undefined ? 'Unavailable' : JSON.stringify(value, null, 2));
const sourceName = (path: string) => path.split(/[\\/]/).slice(-2).join('/');
const status = {
  open: 'Opening report…',
  run: 'Running SHARC rig checks…',
  restore: 'Opening saved DSP report…',
  save: 'Saving DSP to session…',
};
const failure = {
  open: 'Could not open the DSP report.',
  run: 'Could not run the rig report.',
  restore: 'Could not open the saved DSP report.',
  save: 'Could not save DSP to the session.',
};

export default function DspPage(props: {
  active: boolean;
  fs: Pick<FsHost, 'pickFolder' | 'read'>;
  rig?: Host['dspQuality'];
  session?: SavedSession;
  sessionBusy?: boolean;
  saveDsp?(module: string, source?: DspReportSource): Promise<void>;
  openTransfer(): void;
}) {
  const [opened, setOpened] = createSignal<{ root: string; folder?: string; report: QualityReport; sha256: string }>();
  const [operation, setOperation] = createSignal<keyof typeof status>();
  const [moduleId, setModuleId] = createSignal('');
  const [error, setError] = createSignal('');
  const [savedNote, setSavedNote] = createSignal('');
  const [selection, setSelection] = createSignal(0);
  const [catalog, { refetch: refreshCatalog }] = createResource(
    () => (props.active ? props.rig : undefined),
    async (rig) => {
      try {
        return { modules: await rig.listModules() };
      } catch (cause) {
        return { error: `Could not load rig modules. ${cause instanceof Error ? cause.message : String(cause)}` };
      }
    },
  );
  const choices = () => catalog()?.modules ?? [];
  const choiceAvailable = () => choices().some((choice) => choice.id === moduleId());
  const canRun = () => !!props.rig && !catalog.loading && choiceAvailable();
  const report = () => opened()?.report;
  const mismatch = () => !!report() && !!moduleId() && report()?.module !== moduleId();
  const reader = (folder?: string): Pick<FsHost, 'read'> =>
    folder ? { read: (root, file, head) => props.fs.read(root, `${folder}/${file}`, head) } : props.fs;
  const comparison = createMemo(() => {
    const current = opened();
    const pair = current?.report.comparisons[selection()];
    if (current && pair) return { root: current.root, fs: reader(current.folder), pair };
  });
  const totals = createMemo(() => {
    const cases = report()?.files.flatMap((file) => file.cases) ?? [];
    const checks = cases.flatMap((entry) => entry.checks);
    return {
      cases: cases.length,
      passedCases: cases.filter((entry) => entry.status === 'pass').length,
      checks: checks.length,
      passedChecks: checks.filter((check) => check.ok).length,
    };
  });
  let request = 0;
  onCleanup(() => ++request);

  async function perform(kind: keyof typeof status, work: (current: () => boolean) => Promise<void>) {
    if (operation()) return;
    const id = ++request;
    const session = props.session;
    const current = () => id === request && session === props.session;
    setOperation(kind);
    setError('');
    setSavedNote('');
    try {
      await work(current);
    } catch (cause) {
      if (current()) setError(`${failure[kind]} ${cause instanceof Error ? cause.message : String(cause)}`);
    } finally {
      if (current()) setOperation(undefined);
    }
  }

  async function load(root: string, current: () => boolean, folder?: string, module?: string) {
    if (!current()) return;
    const next = await readQualityReport(reader(folder), root, folder?.slice('dsp/'.length));
    if (!current()) return;
    if (module && next.report.module !== module) throw new Error(`Expected a report for ${module}.`);
    batch(() => {
      if (!moduleId()) setModuleId(next.report.module);
      setSelection(0);
      setOpened({ root, folder, ...next });
    });
  }

  createEffect(
    on(
      () => props.session,
      (session) => {
        ++request;
        const dsp = session?.document.dsp;
        batch(() => {
          setOpened(undefined);
          setModuleId(dsp?.module ?? '');
          setOperation(undefined);
          setError('');
          setSavedNote('');
        });
        if (session && dsp?.reportFolder)
          void perform('restore', (current) => load(session.root, current, dsp.reportFolder, dsp.module));
      },
    ),
  );

  function openReport(kind: 'open' | 'run') {
    return perform(kind, async (current) => {
      const root =
        kind === 'run'
          ? (await props.rig?.run(moduleId()))?.folder
          : await props.fs.pickFolder({
              start: opened()?.root ?? null,
              title: 'Choose a DSP quality report folder',
            });
      if (root) await load(root, current);
    });
  }

  function saveToSession() {
    const save = props.saveDsp;
    const module = moduleId();
    if (!save || !module || props.sessionBusy) return;
    const report = opened();
    const source =
      report?.report.module === module && !report.folder ? { folder: report.root, sha256: report.sha256 } : undefined;
    return perform('save', async (current) => {
      await save(module, source);
      if (current()) setSavedNote(source ? 'DSP report saved to session.' : 'DSP selection saved to session.');
    });
  }

  return (
    <Show when={props.active}>
      <main class="dsp-page" aria-label="DSP quality">
        <header class="dsp-heading">
          <h2>DSP quality</h2>
          <div class="dsp-actions">
            <button
              type="button"
              class="btn primary sm"
              disabled={!!operation()}
              onClick={() => void openReport('open')}
            >
              Open report folder
            </button>
            <button type="button" class="btn sm" onClick={() => props.openTransfer()}>
              Install .dkm in Transfer
            </button>
          </div>
        </header>
        <Show when={props.rig || props.session}>
          <form
            class="dsp-run dsp-actions"
            onSubmit={(event) => {
              event.preventDefault();
              if (canRun()) void openReport('run');
            }}
          >
            <label>
              Module
              <select
                value={moduleId()}
                disabled={!!operation()}
                onChange={(event) => {
                  setModuleId(event.currentTarget.value);
                  setSavedNote('');
                }}
              >
                <option value="" disabled selected={!moduleId()}>
                  Choose a module
                </option>
                <Show when={moduleId() && !choiceAvailable()}>
                  <option value={moduleId()} disabled selected>
                    {moduleId()}
                    {catalog.state === 'ready' && !catalog()?.error ? ' (unavailable)' : ''}
                  </option>
                </Show>
                <For each={choices()}>
                  {(choice) => (
                    <option value={choice.id} selected={choice.id === moduleId()}>
                      {choice.name} ({choice.id})
                    </option>
                  )}
                </For>
              </select>
            </label>
            <Show when={props.rig}>
              <button type="submit" class="btn sm" disabled={!!operation() || !canRun()}>
                Run rig report
              </button>
              <button type="button" class="btn sm" disabled={catalog.loading} onClick={() => void refreshCatalog()}>
                Refresh modules
              </button>
            </Show>
            <Show when={props.session && props.saveDsp}>
              <button
                type="button"
                class="btn sm"
                disabled={!!operation() || !!props.sessionBusy || !moduleId()}
                onClick={() => void saveToSession()}
              >
                Save to session
              </button>
            </Show>
          </form>
        </Show>
        <Show when={props.rig}>
          <Show when={catalog.loading}>
            <p role="status">Loading rig modules…</p>
          </Show>
          <Show when={!catalog.loading && catalog()?.error}>
            <p class="dsp-fail" role="alert">
              {catalog()?.error}
            </p>
          </Show>
          <Show when={!catalog.loading && catalog()?.modules?.length === 0}>
            <p role="status">No machines or effects with declarative rig checks found.</p>
          </Show>
        </Show>
        <Show when={(props.rig || props.session) && mismatch()}>
          <p>
            The open report is for {report()?.module}.
            <Show when={props.session}>{` Saving ${moduleId()} stores the module selection only.`}</Show>
          </p>
        </Show>
        <Show when={operation()}>{(kind) => <p role="status">{status[kind()]}</p>}</Show>
        <Show when={savedNote()}>
          <p role="status">{savedNote()}</p>
        </Show>
        <Show when={error()}>
          <p class="dsp-fail" role="alert">
            {error()}
          </p>
        </Show>
        <Show
          when={report()}
          keyed
          fallback={
            <p>
              Choose the output folder containing <code>quality.json</code>.
            </p>
          }
        >
          {(quality) => (
            <>
              <section class="dsp-card" aria-label="Report summary">
                <div class="dsp-heading">
                  <h3>{quality.module}</h3>
                  <strong class={`dsp-${quality.outcome}`}>{quality.outcome.toUpperCase()}</strong>
                </div>
                <p>
                  {quality.scope} · {quality.sampleRate} Hz
                </p>
                <p>
                  {totals().passedCases} / {totals().cases} cases passed · {totals().passedChecks} / {totals().checks}{' '}
                  checks passed
                </p>
                <Show when={quality.diagnostics.length || quality.mediaDiagnostics?.length}>
                  <ul aria-label="Report diagnostics">
                    <For each={quality.diagnostics}>{(line) => <li>{line}</li>}</For>
                    <For each={quality.mediaDiagnostics}>{(line) => <li>{line}</li>}</For>
                  </ul>
                </Show>
              </section>
              <Show when={quality.comparisons.length}>
                <section class="dsp-card" aria-label="Listening comparisons">
                  <h3>Listening comparisons</h3>
                  <label class="dsp-select">
                    Comparison
                    <select value={selection()} onChange={(event) => setSelection(Number(event.currentTarget.value))}>
                      <For each={quality.comparisons}>
                        {(pair, index) => (
                          <option value={index()}>
                            {index() + 1}. {sourceName(pair.baseline.file)} → {sourceName(pair.candidate.file)}
                          </option>
                        )}
                      </For>
                    </select>
                  </label>
                  <Show when={comparison()} keyed>
                    {({ root, fs, pair }) => <ComparisonView fs={fs} root={root} comparison={pair} />}
                  </Show>
                </section>
              </Show>
              <Show when={quality.files.length}>
                <section class="dsp-card dsp-checks" aria-label="Rig cases">
                  <h3>Rig cases</h3>
                  <For each={quality.files}>
                    {(file) => (
                      <section>
                        <h4>{file.file}</h4>
                        <Show when={file.message}>
                          <p class="dsp-fail">{file.message}</p>
                        </Show>
                        <For each={file.cases}>
                          {(entry) => (
                            <details>
                              <summary>
                                <strong class={`dsp-${entry.status}`}>{entry.status.toUpperCase()}</strong>
                                <span>
                                  {entry.case} · {entry.device ?? 'shared rig'} · {entry.tap}
                                </span>
                                <span class="dsp-count">{entry.checks.length} checks</span>
                              </summary>
                              <Show when={entry.message}>
                                <p>{entry.message}</p>
                              </Show>
                              <For each={entry.checks}>
                                {(check) => (
                                  <details>
                                    <summary>
                                      <strong class={check.ok ? 'dsp-pass' : 'dsp-fail'}>
                                        {check.ok ? 'PASS' : 'FAIL'}
                                      </strong>
                                      {check.kind}
                                    </summary>
                                    <Show when={check.message}>
                                      <p>{check.message}</p>
                                    </Show>
                                    <dl class="dsp-measurements">
                                      <div>
                                        <dt>Observed</dt>
                                        <dd>
                                          <pre>{valueText(check.observed)}</pre>
                                        </dd>
                                      </div>
                                      <div>
                                        <dt>Expected</dt>
                                        <dd>
                                          <pre>{valueText(check.expected)}</pre>
                                        </dd>
                                      </div>
                                    </dl>
                                  </details>
                                )}
                              </For>
                            </details>
                          )}
                        </For>
                      </section>
                    )}
                  </For>
                </section>
              </Show>
            </>
          )}
        </Show>
      </main>
    </Show>
  );
}

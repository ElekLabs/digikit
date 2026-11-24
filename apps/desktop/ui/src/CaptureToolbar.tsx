import { For, Show } from 'solid-js';
import type { PerformanceCapture } from '@digi/workbench-core/profile/performance-capture.ts';
import RecordingButton from './RecordingButton';
import type { createSavedPerformance } from './saved-performance';
import type { SpanRecording } from './span-recording';

export default function CaptureToolbar(props: {
  recording?: SpanRecording;
  activeTarget: boolean;
  saved: ReturnType<typeof createSavedPerformance>;
  onSelect(capture: PerformanceCapture): void;
  showProfile(): void;
}) {
  let input!: HTMLInputElement;
  const current = () => props.saved.current();
  const running = () => !!props.recording?.running();
  const live = () => !current() || running();

  return (
    <div class="capture-toolbar">
      <div class="span-actions" role="group" aria-label="Capture controls">
        <Show when={live()}>
          <RecordingButton
            running={running()}
            disabled={!props.activeTarget || !props.recording?.available() || props.recording.disabled()}
            includeDsp={!!props.recording?.includeDsp()}
            dspAvailable={!!props.recording?.dspAvailable()}
            setIncludeDsp={(value) => props.recording?.setIncludeDsp(value)}
            includeSharc={!!props.recording?.includeSharc()}
            sharcAvailable={!!props.recording?.sharcAvailable()}
            setIncludeSharc={(value) => props.recording?.setIncludeSharc(value)}
            onToggle={() => props.recording?.toggle()}
          />
          <button
            type="button"
            disabled={!props.activeTarget || !props.recording?.canSave()}
            onClick={() => void props.recording?.save()}
          >
            Save capture…
          </button>
          <button
            type="button"
            disabled={!props.activeTarget || !props.recording?.available() || props.recording.disabled()}
            onClick={() => void props.recording?.reset()}
          >
            Reset
          </button>
        </Show>
        <div class="capture-source" role="group" aria-label="Capture source">
          <input
            ref={input}
            type="file"
            accept=".json,application/json"
            hidden
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              event.currentTarget.value = '';
              void props.saved.open(file).then((capture) => {
                if (capture && props.saved.current() === capture) props.onSelect(capture);
              });
            }}
          />
          <button type="button" disabled={running()} onClick={() => input.click()}>
            Open capture…
          </button>
          <Show when={props.saved.file()}>
            {(captures) => (
              <>
                <Show when={captures().length > 1} fallback={<span title={current()?.label}>{current()?.label}</span>}>
                  <label>
                    Capture{' '}
                    <select
                      value={props.saved.index()}
                      onChange={(event) => {
                        const capture = props.saved.select(Number(event.currentTarget.value));
                        if (capture) props.onSelect(capture);
                      }}
                    >
                      <For each={captures()}>
                        {(capture, index) => <option value={index()}>{capture.label}</option>}
                      </For>
                    </select>
                  </label>
                </Show>
                <Show when={current()?.hasHotspots}>
                  <button type="button" onClick={props.showProfile}>
                    View hotspots
                  </button>
                </Show>
                <button type="button" onClick={props.saved.clear}>
                  Show current target
                </button>
              </>
            )}
          </Show>
        </div>
      </div>
      <Show when={props.saved.error()}>
        <p role="alert">{props.saved.error()}</p>
      </Show>
      <Show when={live() && props.activeTarget && props.recording?.status()}>
        {(status) => <span role="status">{status()}</span>}
      </Show>
    </div>
  );
}

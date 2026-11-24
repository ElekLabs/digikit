import { Show } from 'solid-js';

export default function RecordingButton(props: {
  running: boolean;
  disabled: boolean;
  onToggle(): void;
  includeDsp: boolean;
  dspAvailable: boolean;
  setIncludeDsp(value: boolean): void;
  includeSharc: boolean;
  sharcAvailable: boolean;
  setIncludeSharc(value: boolean): void;
}) {
  return (
    <>
      <button type="button" class="recording-button" disabled={props.disabled} onClick={() => props.onToggle()}>
        <svg
          aria-hidden="true"
          width="12"
          height="12"
          viewBox="0 0 12 12"
          style={{ 'margin-right': '0.45em', 'vertical-align': '-1px' }}
        >
          <Show when={props.running} fallback={<circle cx="6" cy="6" r="5" fill="var(--bad, currentColor)" />}>
            <rect x="1" y="1" width="10" height="10" rx="1" fill="currentColor" />
          </Show>
        </svg>
        {props.running ? 'Stop recording' : 'Start recording'}
      </button>
      <Show when={props.dspAvailable}>
        <label class="recording-option">
          <input
            type="checkbox"
            checked={props.includeDsp}
            disabled={props.disabled || props.running}
            onChange={(event) => props.setIncludeDsp(event.currentTarget.checked)}
          />
          Include DSP block costs
        </label>
      </Show>
      <Show when={props.sharcAvailable}>
        <label class="recording-option">
          <input
            type="checkbox"
            checked={props.includeSharc}
            disabled={props.disabled || props.running}
            onChange={(event) => props.setIncludeSharc(event.currentTarget.checked)}
          />
          Include SHARC hotspots
        </label>
      </Show>
    </>
  );
}

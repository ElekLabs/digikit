import { Show } from 'solid-js';

export default function RecordingButton(props: {
  running: boolean;
  disabled: boolean;
  onToggle(): void;
  class?: string;
}) {
  return (
    <button type="button" class={props.class} disabled={props.disabled} onClick={() => props.onToggle()}>
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
  );
}

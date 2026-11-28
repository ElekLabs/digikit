import { For, Show, createSignal, onCleanup, untrack } from 'solid-js';
import type { Stereo } from '../analyzer/frame';

const tracks = [0, 1] as const;
const labels = ['A · Baseline', 'B · Candidate'] as const;

export default function ComparisonAudio(props: { urls: Stereo<string | undefined>; position(seconds: number): void }) {
  const [active, setActive] = createSignal<0 | 1>(untrack(() => (props.urls[0] ? 0 : 1)));
  const [time, setTime] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [durations, setDurations] = createSignal<Stereo<number>>([0, 0]);
  const [failed, setFailed] = createSignal<Stereo<boolean>>([false, false]);
  const [error, setError] = createSignal('');
  const audio: [HTMLAudioElement?, HTMLAudioElement?] = [];
  let revision = 0;

  const duration = () => durations()[active()];

  function position(seconds: number) {
    setTime(seconds);
    props.position(seconds);
  }

  function pause() {
    revision++;
    audio[active()]?.pause();
    setPlaying(false);
  }

  async function play(element: HTMLAudioElement) {
    const request = ++revision;
    setError('');
    setPlaying(true);
    try {
      await element.play();
    } catch {
      if (request !== revision) return;
      setPlaying(false);
      setError('The WAV could not be played.');
    }
  }

  function select(index: 0 | 1) {
    if (index === active()) return;
    const next = audio[index];
    if (!next || !durations()[index]) return;
    const previous = audio[active()];
    const resume = playing();
    const seconds = previous?.currentTime ?? time();
    pause();
    setError('');
    setActive(index);
    next.currentTime = Math.min(seconds, next.duration);
    position(next.currentTime);
    if (resume && seconds < next.duration) void play(next);
  }

  function toggle() {
    const element = audio[active()];
    if (!element || !duration()) return;
    if (playing()) pause();
    else {
      if (element.currentTime >= element.duration) {
        element.currentTime = 0;
        position(0);
      }
      void play(element);
    }
  }

  onCleanup(() => {
    revision++;
    for (const element of audio) {
      if (!element) continue;
      element.pause();
      element.removeAttribute('src');
      element.load();
    }
  });

  return (
    <Show when={props.urls.some(Boolean)}>
      <div class="dsp-audio">
        <div class="dsp-actions" role="group" aria-label="A/B listening">
          <button type="button" class="btn sm" disabled={!duration() || failed()[active()]} onClick={toggle}>
            {playing() ? 'Pause' : 'Play'}
          </button>
          <For each={tracks}>
            {(index) => (
              <>
                <button
                  type="button"
                  class="btn sm"
                  classList={{ primary: active() === index }}
                  disabled={!durations()[index] || failed()[index]}
                  aria-pressed={active() === index}
                  onClick={() => select(index)}
                >
                  {labels[index]}
                </button>
                <Show when={props.urls[index]}>
                  {(url) => (
                    <audio
                      ref={(element) => (audio[index] = element)}
                      preload="metadata"
                      src={url()}
                      onLoadedMetadata={(event) => {
                        const length = event.currentTarget.duration;
                        if (!Number.isFinite(length) || length <= 0) return;
                        setDurations(([a, b]) => (index === 0 ? [length, b] : [a, length]));
                      }}
                      onTimeUpdate={(event) => {
                        if (index === active()) position(event.currentTarget.currentTime);
                      }}
                      onEnded={() => {
                        if (index === active()) setPlaying(false);
                      }}
                      onError={() => {
                        setFailed(([a, b]) => (index === 0 ? [true, b] : [a, true]));
                        if (index === active()) pause();
                      }}
                    />
                  )}
                </Show>
              </>
            )}
          </For>
        </div>
        <label class="dsp-playhead">
          <span>Position</span>
          <input
            type="range"
            min="0"
            max={duration() || 1}
            step="0.01"
            value={time()}
            aria-valuetext={`${time().toFixed(2)} seconds`}
            disabled={!duration() || failed()[active()]}
            onInput={(event) => {
              const element = audio[active()];
              if (element) element.currentTime = event.currentTarget.valueAsNumber;
              position(event.currentTarget.valueAsNumber);
            }}
          />
          <span>
            {time().toFixed(2)} / {duration().toFixed(2)} s
          </span>
        </label>
        <For each={tracks}>
          {(index) => (
            <Show when={failed()[index]}>
              <p role="status">{labels[index]} WAV could not be decoded.</p>
            </Show>
          )}
        </For>
        <Show when={error()}>
          <p role="status">{error()}</p>
        </Show>
      </div>
    </Show>
  );
}

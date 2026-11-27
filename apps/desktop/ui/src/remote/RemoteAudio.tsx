import { For, Show, createEffect, on, onCleanup, onMount, untrack } from 'solid-js';
import type { RemoteAudioHost } from '@digi/workbench-core/host/host.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { createRemoteAudio } from './audio';

export default function RemoteAudio(props: { host: RemoteAudioHost; source: () => TargetBinding }) {
  const app = useTransferApp();
  const audio = createRemoteAudio(
    untrack(() => props.host),
    () => app.env.targets.ports(),
  );
  createEffect(
    on(
      () => [props.source(), app.target.switching()] as const,
      ([source, switching]) => {
        if (switching) return;
        if (source.target.kind !== 'midi') {
          void audio.select();
          return;
        }
        const unbind = source.beforeLeave(audio.release);
        onCleanup(unbind);
        void audio.select(source.target.port);
      },
    ),
  );
  onMount(() => {
    const timer = setInterval(() => void audio.check(), 1000);
    onCleanup(() => clearInterval(timer));
  });
  onCleanup(() => void audio.select());
  return (
    <Show when={props.source().target.kind === 'midi'}>
      <span class="remote-audio">
        <label>
          USB audio{' '}
          <select
            aria-label="Remote audio input"
            disabled={app.target.switching()}
            value={audio.state().choice}
            onChange={(event) => {
              if (event.currentTarget.value === 'refresh') {
                event.currentTarget.value = audio.state().choice;
                void audio.refresh();
              } else void audio.choose(event.currentTarget.value);
            }}
          >
            <option value="auto">Auto</option>
            <option value="off">Off</option>
            <For each={audio.state().inputs}>
              {(input) => (
                <option value={input.id}>
                  {input.name}
                  {audio.state().inputs.some((other) => other !== input && other.name === input.name)
                    ? ` — ${input.id}`
                    : ''}
                </option>
              )}
            </For>
            <option value="refresh">Rescan inputs…</option>
          </select>
        </label>
        <Show
          when={audio.state().error}
          fallback={
            <span>{audio.state().output ?? (audio.state().choice === 'auto' ? 'Choose an input if needed' : '')}</span>
          }
        >
          {(error) => (
            <span class="warn" role="status">
              {error()}
            </span>
          )}
        </Show>
      </span>
    </Show>
  );
}

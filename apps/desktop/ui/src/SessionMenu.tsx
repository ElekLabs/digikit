import { For, Show, untrack } from 'solid-js';
import type { createWorkspaceSession } from './workspace-session';
import './session-menu.css';

export default function SessionMenu(props: {
  session: ReturnType<typeof createWorkspaceSession>;
  ref?: (node: HTMLDetailsElement) => void;
}) {
  const s = untrack(() => props.session);
  return (
    <details class="workbench-session" ref={props.ref}>
      <summary title={s.name()}>
        Session <span class="session-name">{s.name()}</span>
        <Show when={s.card()}>
          <span title="Working card changes are saved only on request">· Working</span>
        </Show>
        <Show when={s.saved() && !s.card()}>
          <span>· Not mounted</span>
        </Show>
      </summary>
      <section class="workbench-session-menu" aria-label="Session">
        <div class="session-actions">
          <button disabled={s.disabled()} onClick={() => void s.newSession()}>
            New…
          </button>
          <button disabled={s.disabled()} onClick={() => void s.open()}>
            Open…
          </button>
          <button disabled={s.disabled()} onClick={() => void s.save()}>
            Save
          </button>
          <button disabled={s.disabled()} onClick={() => void s.saveAs()}>
            Save As…
          </button>
          <Show when={s.saved() || s.draft()}>
            <button disabled={s.disabled()} onClick={() => void s.close()}>
              Close
            </button>
          </Show>
        </div>
        <Show when={s.saved() || s.draft()}>
          <label>
            Name <input value={s.name()} disabled={s.busy()} onInput={(e) => s.setName(e.currentTarget.value)} />
          </label>
        </Show>
        <Show when={s.draft()}>
          <p>Choose firmware and a card for this session, or use Save As to copy the mounted emulator card.</p>
          <div class="session-actions">
            <button disabled={s.disabled()} onClick={() => void s.create('image')}>
              Use card image…
            </button>
            <button disabled={s.disabled()} onClick={() => void s.create('folder')}>
              Build from +Drive folder…
            </button>
          </div>
        </Show>
        <Show when={s.saved()}>
          {(saved) => (
            <>
              <p title={saved().root}>
                {saved().document.firmware.device.toUpperCase()} · DSP {saved().document.firmware.dsp ? 'on' : 'off'} ·{' '}
                {saved().document.checkpoint ? 'Saved checkpoint' : 'Imported card'}
              </p>
              <div class="session-actions">
                <Show
                  when={s.card()}
                  fallback={
                    <>
                      <button disabled={s.disabled()} onClick={() => void s.start()}>
                        Start emulator
                      </button>
                      <button disabled={s.disabled()} onClick={() => void s.locate()}>
                        Locate firmware…
                      </button>
                    </>
                  }
                >
                  <button disabled={s.disabled()} onClick={() => void s.revert()}>
                    Revert to last save
                  </button>
                </Show>
                <button disabled={s.disabled()} onClick={() => void s.attach()}>
                  Attach file…
                </button>
              </div>
              <Show when={saved().document.attachments.length}>
                <h4>Attachments</h4>
                <For each={saved().document.attachments}>
                  {(item) => (
                    <button disabled={s.disabled()} onClick={() => void s.openAttachment(item)}>
                      Open {item.name}
                    </button>
                  )}
                </For>
              </Show>
            </>
          )}
        </Show>
        <Show when={s.recent().length}>
          <h4>Recent sessions</h4>
          <For each={s.recent()}>
            {(entry) => (
              <button title={entry.root} disabled={s.disabled()} onClick={() => void s.open(entry.root)}>
                {entry.name}
              </button>
            )}
          </For>
        </Show>
        <Show when={s.busy()}>
          <p role="status">Working…</p>
        </Show>
        <Show when={s.note()}>
          <p role="status">{s.note()}</p>
        </Show>
        <Show when={s.error()}>
          <p role="alert">{s.error()}</p>
        </Show>
      </section>
    </details>
  );
}

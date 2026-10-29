// The inside of the Log drawer: the header (Source, Errors only, Follow, Save, Copy, Clear) and the list. The dock around it (its height and
// its handle) is in Console.tsx, and the preview puts it in a dock of its own. The rules are in log-core.ts, the runtime logs in log-follow.ts.
import { invoke } from '@tauri-apps/api/core';
import { For, Show, createEffect, createMemo, createSignal, onCleanup } from 'solid-js';
import { copyText, filterEntries, formatTime, saveName, saveText, sourceMenu, type Origin } from './log-core';
import type { LogFollow } from './log-follow';
import type { OriginStatus } from './log-follow-core';
import { clearLog, followOn, logOpen, logState, setFollowOn } from './log-state';

export interface LogPanelProps {
  follow: LogFollow;
  /** Something is there to follow: the emulator runs, the MIDI port was chosen. */
  emulatorRunning(): boolean;
  devicePort(): string | undefined;
}

const NO_RUNTIME = 'The selected target has no digikit runtime';
const followable = (running: boolean, status: OriginStatus) =>
  running && status !== 'no-runtime' && status !== 'absent';

export default function LogPanel(props: LogPanelProps) {
  const [logSource, setLogSource] = createSignal('all');
  const [errorsOnly, setErrorsOnly] = createSignal(false);
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(new Set<number>());
  const [note, setNote] = createSignal('');
  let logList: HTMLOListElement | undefined;
  let stickToEnd = true;
  const timers: number[] = [];
  onCleanup(() => timers.forEach((t) => window.clearTimeout(t)));

  const visibleLog = createMemo(() =>
    filterEntries(logState().entries, { source: logSource(), errorsOnly: errorsOnly() }),
  );
  const live = (origin: Origin) =>
    origin === 'emulator'
      ? followable(props.emulatorRunning(), props.follow.status().emulator)
      : followable(!!props.devicePort(), props.follow.status().device);
  const menu = createMemo(() =>
    sourceMenu({
      emulator: {
        live: live('emulator'),
        logged: logState().entries.some((e) => e.source === 'emulator' && e.rt),
        modules: props.follow.modules().emulator,
      },
      device: {
        port: props.devicePort(),
        live: live('device'),
        logged: logState().entries.some((e) => e.source === 'device'),
        modules: props.follow.modules().device,
      },
    }),
  );
  // a module that went away from the menu cannot stay selected
  createEffect(() => {
    const current = logSource();
    if (current !== 'all' && !menu().some((g) => g.options.some((o) => o.value === current && !o.disabled)))
      setLogSource('all');
  });
  const canFollow = () => live('emulator') || live('device');

  const toggleExpanded = (id: number) =>
    setExpanded((set) => {
      const next = new Set(set);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const flash = (text: string, ms = 1500) => {
    setNote(text);
    timers.push(window.setTimeout(() => setNote(''), ms));
  };
  const copyLog = async () => {
    try {
      await navigator.clipboard.writeText(copyText(visibleLog()));
      flash('Copied');
    } catch {
      /* the clipboard may be blocked; the entries stay on screen */
    }
  };
  const saveLog = async () => {
    try {
      const bytes = new TextEncoder().encode(saveText(visibleLog(), Date.now()));
      const where = await invoke<string | null>('log_save', bytes, {
        headers: { 'x-name': encodeURIComponent(saveName(Date.now())) },
      });
      if (where) flash('Saved', 2500);
    } catch {
      flash('Save failed', 3000);
    }
  };

  // newest at the bottom: follow it while the list is at the bottom
  createEffect(() => {
    visibleLog();
    if (logOpen())
      queueMicrotask(() => {
        if (logList && stickToEnd) logList.scrollTop = logList.scrollHeight;
      });
  });

  return (
    <>
      <header>
        <strong>Log</strong>
        <span class="count">
          {visibleLog().length === logState().entries.length
            ? `${logState().entries.length} entries`
            : `${visibleLog().length} of ${logState().entries.length}`}
        </span>
        <span class="sp" />
        <select aria-label="Source" value={logSource()} onChange={(e) => setLogSource(e.currentTarget.value)}>
          <option value="all">All</option>
          <For each={menu()}>
            {(group) => (
              <optgroup label={group.label}>
                <For each={group.options}>
                  {(o) => (
                    <option value={o.value} disabled={o.disabled} title={o.title} selected={o.value === logSource()}>
                      {o.label}
                    </option>
                  )}
                </For>
              </optgroup>
            )}
          </For>
        </select>
        <button
          type="button"
          class="tb small"
          classList={{ on: errorsOnly() }}
          aria-pressed={errorsOnly()}
          onClick={() => setErrorsOnly(!errorsOnly())}
        >
          Errors only
        </button>
        <button
          type="button"
          class="tb small"
          classList={{ on: followOn() && canFollow() }}
          aria-pressed={followOn() && canFollow()}
          disabled={!canFollow()}
          title={canFollow() ? 'Read the runtime log of the emulator and of the device on the MIDI port' : NO_RUNTIME}
          onClick={() => setFollowOn(!followOn())}
        >
          Follow
          <Show when={followOn() && canFollow()}>
            <i class="follow-dot" aria-hidden="true" />
          </Show>
        </button>
        <button
          type="button"
          class="tb small"
          disabled={visibleLog().length === 0}
          title="Save the entries shown to a file (the format of dklog follow)"
          onClick={() => void saveLog()}
        >
          {note() === 'Saved' || note() === 'Save failed' ? note() : 'Save'}
        </button>
        <button
          type="button"
          class="tb small"
          disabled={visibleLog().length === 0}
          title="Copy the entries shown, with their details"
          onClick={() => void copyLog()}
        >
          {note() === 'Copied' ? 'Copied' : 'Copy'}
        </button>
        <button
          type="button"
          class="tb small"
          disabled={logState().entries.length === 0}
          onClick={() => {
            clearLog();
            setExpanded(new Set<number>());
          }}
        >
          Clear
        </button>
      </header>
      <ol
        class="log-list"
        ref={logList}
        onScroll={(e) => {
          const el = e.currentTarget;
          stickToEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
        }}
      >
        <For
          each={visibleLog()}
          fallback={
            <li class="empty">{logState().entries.length ? 'No entry matches the filter.' : 'Nothing logged yet.'}</li>
          }
        >
          {(entry) => (
            <li classList={{ bad: entry.level === 'error' }}>
              <div class="row">
                <time>{formatTime(entry.at)}</time>
                <span class="src">
                  {entry.source}
                  {entry.level === 'error' ? ' ✕' : ''}
                </span>
                <span class="text" title={entry.text}>
                  {entry.text}
                </span>
                <Show when={entry.detail}>
                  <button
                    type="button"
                    class="more"
                    aria-expanded={expanded().has(entry.id)}
                    aria-label={expanded().has(entry.id) ? 'Hide the full output' : 'Show the full output'}
                    title={expanded().has(entry.id) ? 'Hide the full output' : 'Show the full output'}
                    onClick={() => toggleExpanded(entry.id)}
                  >
                    {expanded().has(entry.id) ? '▾' : '▸'}
                  </button>
                </Show>
              </div>
              <Show when={entry.detail && expanded().has(entry.id)}>
                <pre class="detail">{entry.detail}</pre>
              </Show>
            </li>
          )}
        </For>
      </ol>
    </>
  );
}

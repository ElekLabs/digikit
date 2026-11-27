// The Transfer page, shared by the desktop workbench and the site (the app gives its parts in env.ts): one page, up to two modes. A dropdown
// picks Modules (the built modules of the library and the card's module store) or, in the desktop app, Samples (a folder of samples and the
// device's sample folders). The left column is always "On your computer", the right
// column "On device"; each column carries its own controls (ModulesMode.tsx, SamplesMode.tsx; the shared parts are in TransferParts.tsx).
// This file holds what the modes share: the top bar (title, mode), the one busy action, the error line, and the progress bar along the bottom.
// There is no Refresh button: the page reads by itself (AutoRefresh.tsx, refresh-core.ts). The page speaks the upload protocol itself, in
// TypeScript, over the controller's USB capture; no process is started and nothing is built.
import { Match, Show, Switch, batch, createEffect, createMemo, createSignal, on, onMount, type JSX } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import Segmented from '../components/Segmented';
import UsageBar, { type UsagePart } from '../components/UsageBar';
import AutoRefresh from './AutoRefresh';
import ConnectionBadge from './ConnectionBadge';
import CopyErrorButton from '../components/CopyErrorButton';
import { useTransferApp } from '../../src/transfer/app-context';
import ModulesMode from './ModulesMode';
import SourcePanel from './SourcePanel';
import TransferBar from './TransferBar';
import { MODES, installBytes, modulesBar, parseMode } from '../../src/transfer/transfer-core';
import { storageBar } from '../../src/transfer/storage-bar';
import { formatBytes } from '../../src/modules/backend';
import { cancellable, cancelJob, runTransfer } from '../../src/transfer/run-transfer';
import { writing, type PageCtx } from './TransferParts';
import './transfer-page.css';

/** `onFlash`: starts the one-click Flash flow from a service refusal (the host gives it: the desktop opens its Firmware page, the site links to its own). */
export default function TransferPage(props: { onFlash?: () => void }): JSX.Element {
  const app = useTransferApp();
  const { env } = app;
  const { target, rescanTargets, switching } = app.target;
  const { connection } = app.connection;
  const {
    activeMode,
    busy,
    deviceInfo,
    error,
    halted,
    jobProgress,
    lastView,
    log,
    noRuntime,
    progress,
    readingAfterJob,
    sampleUsage,
    setDelTicks,
    setDeviceInfo,
    setError,
    setHalted,
    setMode,
    ticks,
  } = app.state;
  const [reload, setReload] = createSignal(0);

  const ctx: PageCtx = { busy, run: (what, f) => runTransfer(app, what, f), setError, reload };
  const readNow = () => {
    setHalted(false);
    void runTransfer(app, 'Reading', async () => {});
  };
  const autoRead = () => {
    if (!halted()) readNow();
  };
  // a change of the emulator's runtime, or a reconnect, lets a halted page read again
  createEffect(
    on(
      app.target.libraryOnly,
      () => {
        if (target().kind === 'emulator') readNow();
      },
      { defer: true },
    ),
  );
  createEffect(
    on(
      () => connection().state,
      (s) => {
        if (s === 'connected' && halted()) readNow();
      },
      { defer: true },
    ),
  );

  // A write reads at its end. An unfinished first Samples listing may have been interrupted while away.
  onMount(() => {
    const job = busy();
    if (
      !job ||
      (job === 'Reading' && activeMode() === 'samples' && !deviceInfo() && env.extras?.samples?.readInterrupted())
    )
      void runTransfer(app, 'Reading', async () => {});
  });
  // Read a new target, or recover a first Samples listing interrupted by a failed switch.
  createEffect(
    on([target, switching], ([selected, changing], previous) => {
      if (!previous || changing) return;
      const [priorTarget, wasChanging] = previous;
      if (selected !== priorTarget) {
        setDeviceInfo(undefined);
        setDelTicks(new Set<string>());
        readNow();
      } else if (wasChanging && activeMode() === 'samples' && !deviceInfo()) {
        readNow();
      }
    }),
  );
  // another mode: the device line is the mode's own (its free space), so read it again
  const changeMode = (value: string) => {
    setMode(parseMode(value));
    setError('');
    setDeviceInfo(undefined);
    void runTransfer(app, 'Reading', async () => {});
  };

  // the bar along the bottom, for the install, delete or restart of Modules and the upload of Samples
  const bar = createMemo(
    () => {
      if (activeMode() === 'modules') {
        const p = progress();
        return p ? { info: modulesBar(p), running: writing(busy()) } : undefined;
      }
      return env.extras?.samples?.bar(busy());
    },
    undefined,
    {
      // a builder may return a fresh object each time: equal fields must not notify
      equals: (a, b) =>
        a === b ||
        (!!a &&
          !!b &&
          a.running === b.running &&
          a.info.fraction === b.info.fraction &&
          a.info.message === b.info.message &&
          a.info.failed === b.info.failed &&
          a.info.sent === b.info.sent &&
          a.info.total === b.info.total &&
          a.info.checking === b.info.checking),
    },
  );
  const samples = () => env.extras?.samples;
  // the storage bar: the kind of the mode against the others and the free space, with what the ticked items add
  const storage = createMemo(() => {
    const v = lastView();
    const usage = activeMode() === 'modules' ? v?.usage : sampleUsage();
    if (!usage) return undefined;
    const added = activeMode() === 'modules' && v ? installBytes(v, ticks()) : (samples()?.pendingBytes?.() ?? 0);
    const b = storageBar(usage, activeMode(), added);
    const parts: UsagePart[] = b.segments.map((x) => ({
      key: x.key,
      bytes: x.bytes,
      title: `${x.label} ${formatBytes(x.bytes)}`,
      legend: `${x.label} ${formatBytes(x.bytes)}`,
    }));
    return { b, parts };
  });
  // the status bar shows the same fraction and line
  createEffect(() => {
    const b = bar();
    if (activeMode() === 'modules' && b?.running) jobProgress(b.info.fraction, b.info.message);
  });

  return (
    <section class="transfer-page" aria-label="Transfer">
      <header class="head">
        <h2>Transfer</h2>
        <Show when={samples()}>
          <Segmented
            label="Mode"
            options={MODES}
            value={activeMode()}
            onChange={(m) => changeMode(m)}
            disabled={writing(busy())}
          />
        </Show>
        <span class="sp" />
        <ConnectionBadge />
        <Show when={busy() === 'Reading'}>
          <span class="meta">{readingAfterJob() ? 'refreshing…' : 'reading…'}</span>
        </Show>
      </header>
      <AutoRefresh
        busy={busy}
        read={autoRead}
        rescanPorts={() => {
          batch(() => {
            setReload((n) => n + 1);
            rescanTargets();
          });
        }}
      />

      <Show when={error()}>
        {(message) => (
          <p class="err" role="alert">
            <span class="msg">{message()}</span>
            <CopyErrorButton text={message()} />
            <button type="button" class="act small" disabled={!!busy()} onClick={readNow}>
              Try again
            </button>
            <Show when={noRuntime() && target().kind === 'emulator' && env.extras?.reloadWithRuntime}>
              <button type="button" class="act small" onClick={() => env.extras?.reloadWithRuntime?.()}>
                Reload with runtime
              </button>
            </Show>
          </p>
        )}
      </Show>

      <Show when={storage()}>
        {(st) => (
          <div class="storage-bar" classList={{ over: st().b.overflow > 0 }}>
            <UsageBar parts={st().parts} base={st().b.base} label={st().b.text} over={st().b.overflow > 0} />
            <p class="storage-text" role="status">
              {st().b.text}
            </p>
          </div>
        )}
      </Show>

      <Switch>
        <Match when={activeMode() === 'modules'}>
          <ModulesMode ctx={ctx} onFlash={props.onFlash} />
        </Match>
        <Match when={activeMode() === 'samples' && samples()}>
          {(slot) => <Dynamic component={slot().Mode} ctx={ctx} />}
        </Match>
      </Switch>

      <Show when={bar()}>
        {(b) => (
          <TransferBar
            bar={b().info}
            running={b().running}
            onCancel={cancellable(busy()) ? () => cancelJob(app) : undefined}
          />
        )}
      </Show>
      <Show when={activeMode() === 'modules' && log().length && !bar()}>
        <pre class="log">{log().join('\n')}</pre>
      </Show>
      <SourcePanel />
    </section>
  );
}

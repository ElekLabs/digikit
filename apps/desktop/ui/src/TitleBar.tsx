// The title bar of the workbench, over every page: the name, then the one target selector the pages share (target.ts). Transfer, Remote, Firmware
// and the Memory panel all talk to the target picked here, so it sits above them rather than in each page (env.ts `targets.inHeader`).
import { onCleanup, onMount, Show, type JSX } from 'solid-js';
import ConnectionBadge from '@digi/workbench-core/ui/transfer/ConnectionBadge.tsx';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import TargetSelect from '@digi/workbench-core/ui/transfer/TargetSelect.tsx';
import { canAutoRefresh, focusReadDue } from '@digi/workbench-core/transfer/refresh-core.ts';

// the title bar reads the ports by itself on every page, slower than the Transfer page's poll
const RESCAN_MS = 10_000;

export default function TitleBar(props: { onError?: (message: string) => void }): JSX.Element {
  const app = useTransferApp();
  const { target, rescanTargets } = app.target;
  const transferBusy = app.state.busy;
  const jobBusy = app.jobs.busy;
  const locked = () => jobBusy('target');
  let lastFocus: number | undefined;
  const free = () => canAutoRefresh(document.visibilityState === 'visible', transferBusy()) && !jobBusy('target');
  const back = () => {
    const now = Date.now();
    if (!free() || !focusReadDue(lastFocus, now)) return;
    lastFocus = now;
    rescanTargets();
  };
  onMount(() => {
    window.addEventListener('focus', back);
    document.addEventListener('visibilitychange', back);
    const timer = window.setInterval(() => free() && rescanTargets(), RESCAN_MS);
    onCleanup(() => {
      window.removeEventListener('focus', back);
      document.removeEventListener('visibilitychange', back);
      window.clearInterval(timer);
    });
  });
  return (
    <header class="titlebar">
      <span class="brand">
        <span class="brand-mark" aria-hidden="true">
          <i />
          <i />
          <i />
          <i />
        </span>
        digikit
        <span class="brand-sub">workbench</span>
      </span>
      <span class="sp" />
      <Show when={target().kind !== 'none'}>
        <ConnectionBadge />
      </Show>
      {/* a running install, upload or firmware flash keeps its target: the selector is locked until it ends */}
      <TargetSelect disabled={locked()} onError={props.onError} />
    </header>
  );
}

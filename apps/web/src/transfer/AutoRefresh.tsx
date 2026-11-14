// The page reads by itself (refresh-core.ts says when): when the window is shown or gets the focus, every few seconds for the folder on the
// computer (a cheap listing), and the MIDI ports are listed again with each tick. Nothing here starts while a job runs. No visible parts.
import { onCleanup, onMount, type JSX } from 'solid-js';
import { useTransferApp } from './app-context';
import { canAutoRefresh, focusReadDue, POLL_MS } from './refresh-core';
import { errorLine } from '../modules/backend';

export default function AutoRefresh(props: {
  busy: () => string;
  /** Read both sides now. */
  read: () => void;
  /** The target selector lists the MIDI ports again. */
  rescanPorts: () => void;
}): JSX.Element | null {
  const { env, state } = useTransferApp();
  const { activeMode, setError } = state;
  let lastFocusRead: number | undefined;
  const visible = () => document.visibilityState === 'visible';
  const back = () => {
    if (!canAutoRefresh(visible(), props.busy())) return;
    const now = Date.now();
    if (!focusReadDue(lastFocusRead, now)) return;
    lastFocusRead = now;
    props.rescanPorts();
    props.read();
  };
  let polling = false;
  const poll = async () => {
    if (polling || !canAutoRefresh(visible(), props.busy())) return;
    polling = true;
    try {
      props.rescanPorts();
      const samples = env.extras?.samples;
      if (activeMode() !== 'samples' || !samples) return;
      const changed = await samples.localChanged();
      // a job may have started while the listing ran: leave it alone
      if (changed && activeMode() === 'samples' && canAutoRefresh(visible(), props.busy())) await samples.readLocal();
    } catch (e) {
      setError(errorLine(e));
    } finally {
      polling = false;
    }
  };
  onMount(() => {
    window.addEventListener('focus', back);
    document.addEventListener('visibilitychange', back);
    const timer = window.setInterval(() => void poll(), POLL_MS);
    onCleanup(() => {
      window.removeEventListener('focus', back);
      document.removeEventListener('visibilitychange', back);
      window.clearInterval(timer);
    });
  });
  return null;
}

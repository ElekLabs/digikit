// The connection indicator in the header of the Transfer page: a dot and "Connected" / "Connecting…" / "Not connected", then, when connected, the
// device, its runtime and its id, and the port. Rules and texts: connection-core.ts. The full text is the tooltip.
import { For, type JSX } from 'solid-js';
import { connectionView } from './connection-core';
import { useTransferApp } from './app-context';
import { targetLabel } from './target';
import './connection-badge.css';

export default function ConnectionBadge(): JSX.Element {
  const app = useTransferApp();
  const { connection } = app.connection;
  const { target } = app.target;
  const view = () => connectionView(connection(), targetLabel(target()));
  return (
    <span class="conn" classList={{ [view().state]: true }} role="status" aria-label="Connection" title={view().title}>
      <span class="dot" aria-hidden="true" />
      <span class="word">{view().label}</span>
      <For each={view().parts}>{(p) => <span class="part">{p}</span>}</For>
    </span>
  );
}

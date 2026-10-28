// The Transfer page of the site: asks for Web MIDI access (SysEx), picks the first device port, then shows the shared pane (TransferPage.tsx).
// Without Web MIDI (Safari) or without the SysEx permission the pane still opens on the `none` target; its error line says why (web-env.ts, `whyNone`)
// and its Try again button asks again.
import { Show, createSignal, onMount, type JSX } from 'solid-js';
import TransferPage from './TransferPage';
import { startWebTransfer } from './web-env';

export default function WebTransfer(): JSX.Element {
  const [ready, setReady] = createSignal(false);
  onMount(() => {
    // a failure leaves the target on none: the pane shows the reason
    void startWebTransfer()
      .catch(() => {})
      .finally(() => setReady(true));
  });
  return (
    <Show when={ready()}>
      <TransferPage />
    </Show>
  );
}

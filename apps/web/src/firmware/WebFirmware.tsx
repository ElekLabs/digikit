// The Firmware page of the site: asks for Web MIDI access (SysEx), picks the first device port, then shows the shared page (FirmwarePage.tsx).
// Without Web MIDI (Safari) or without the SysEx permission the page still opens and works up to the checks; its device step says why there is
// no port.
import { Show, createSignal, onMount, type JSX } from 'solid-js';
import { startWebTransfer } from '../transfer/web-env';
import FirmwarePage from './FirmwarePage';
import './web-env';

export default function WebFirmware(): JSX.Element {
  const [ready, setReady] = createSignal(false);
  onMount(() => {
    // a failure leaves the target on none: the device step shows the reason
    void startWebTransfer()
      .catch(() => {})
      .finally(() => setReady(true));
  });
  return (
    <Show when={ready()}>
      <FirmwarePage />
    </Show>
  );
}

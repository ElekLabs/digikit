// The Firmware page of the site: asks for Web MIDI access (SysEx), picks the first device port, then shows the shared page (FirmwarePage.tsx).
// Without Web MIDI (Safari) or without the SysEx permission the page still opens and works up to the checks; its device step says why there is
// no port. The island builds the site's apps once and gives them to the page (AppProvider).
import { Show, createSignal, onMount, type JSX } from 'solid-js';
import AppProvider from '@digi/workbench-core/ui/transfer/AppProvider.tsx';
import { createWebFirmwareApp } from './web-env';
import FirmwarePage from '@digi/workbench-core/ui/firmware/FirmwarePage.tsx';

export default function WebFirmware(): JSX.Element {
  const web = createWebFirmwareApp();
  const [ready, setReady] = createSignal(false);
  onMount(() => {
    // a failure leaves the target on none: the device step shows the reason
    void web
      .start()
      .catch(() => {})
      .finally(() => setReady(true));
  });
  return (
    <Show when={ready()}>
      <AppProvider app={web.app} firmware={web.firmware}>
        <FirmwarePage />
      </AppProvider>
    </Show>
  );
}

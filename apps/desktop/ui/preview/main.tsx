// The preview entry: the desktop shell (top bar, sidebar, content) around the real Transfer page, over the fake device of sim.ts.
// Mirrors the markup of ../src/Console.tsx for the parts around the page, so widths and heights are the app's.
import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@web/styles/site.css';
import '../src/console.css';
import '../src/native.css';
import '../src/desktop-env';
import { installNativeFeel } from '../src/native';
import LivePanel from '@web/components/LivePanel';
import { panels } from '@web/data/faceplates';
import FirmwarePage from '@web/firmware/FirmwarePage';
import RemotePage from '../src/RemotePage';
import SidePanels from '../src/SidePanels';
import { setLocalFolder } from '../src/samples-state';
import TransferPage from '@web/transfer/TransferPage';
import { setMode } from '@web/transfer/transfer-state';
import { cardDevice, reloadCard } from './card-mock';
import { installRemoteMock, testPattern } from './remote-mock';
import { createLogFollow } from '../src/log-follow';
import LogPanel from '../src/LogPanel';
import { setLogOpen } from '../src/log-state';
import { logMode, previewLog } from './runtime-log-mock';
import { SAMPLES_ROOT, control, sim } from './sim';

// ?page=transfer (default) | remote | emulator | firmware (state: fwdev, fwstate, fwos; see firmware-mock.ts): the Emulator page shows the faceplate only (the real one needs the emulator) with the same
// markup as ../src/Console.tsx, to put its screen next to the Remote page's.
const page = new URLSearchParams(location.search).get('page') ?? 'transfer';
if (page === 'remote') installRemoteMock();
const emulatorFaceplate = () => (
  <div class="emu-row">
    <main class="stage" aria-label="front panel">
      <div class="surface">
        <LivePanel
          panel={panels[cardDevice]}
          runtime={() => undefined}
          onError={() => {}}
          ref={(api) => queueMicrotask(() => api.showFrame(testPattern()))}
        />
      </div>
    </main>
    <SidePanels
      visible
      card
      reload={reloadCard}
      session={() => Promise.resolve({} as never)}
      where={() => 'the emulator'}
    />
  </div>
);

/** The Log drawer under an empty page, as in the app: the same panel, fed by the fake runtime logs. */
function LogPage() {
  setLogOpen(true);
  const follow = createLogFollow({
    follow: () => true,
    flashing: () => false,
    emulator: { running: previewLog.emulatorRunning, session: async () => previewLog.emulator },
    device: { port: previewLog.devicePort, session: async () => previewLog.device, present: async () => true },
  });
  return (
    <section
      class="log-drawer"
      aria-label="Log"
      style={{ height: `${Number(new URLSearchParams(location.search).get('logh') ?? 360)}px` }}
    >
      <LogPanel follow={follow} emulatorRunning={previewLog.emulatorRunning} devicePort={previewLog.devicePort} />
    </section>
  );
}

installNativeFeel();
setMode(sim.mode);
setLocalFolder(sim.folder === 'none' ? '' : SAMPLES_ROOT);
(window as unknown as { __sim: typeof control }).__sim = control;

render(
  () => (
    <div class="console" style={{ '--accent': panels.dn2.accent, '--func-legend': panels.dn2.func_legend }}>
      <header class="topbar">
        <span class="path">
          UI preview: fake device (state in the address: ?dev=ok|empty|noreply|error|slow &amp; folder=files|empty|none
          &amp; speed=MiB/s &amp; mode=modules|samples &amp; mkdir=0|1)
        </span>
      </header>
      <div class="shell">
        <nav class="sidebar" aria-label="Pages">
          <button type="button" class="side toggle" aria-label="Collapse the sidebar">
            <span>«</span>
          </button>
          <button type="button" class={page === 'emulator' ? 'side on' : 'side'}>
            <span>Emulator</span>
          </button>
          <button type="button" class={page === 'transfer' ? 'side on' : 'side'}>
            <span>Transfer</span>
          </button>
          <button type="button" class={page === 'remote' ? 'side on' : 'side'}>
            <span>Remote</span>
          </button>
          <button type="button" class={page === 'firmware' ? 'side on' : 'side'}>
            <span>Firmware</span>
          </button>
        </nav>
        <div class="content">
          {page === 'log' ? (
            <p class="note" style={{ padding: '24px' }}>
              Log preview ({logMode}): the drawer below follows the fake emulator and the fake device.
            </p>
          ) : page === 'remote' ? (
            <RemotePage fallbackDevice="dn2" />
          ) : page === 'emulator' ? (
            emulatorFaceplate()
          ) : page === 'firmware' ? (
            <FirmwarePage />
          ) : (
            <TransferPage />
          )}
        </div>
      </div>
      {page === 'log' ? <LogPage /> : null}
    </div>
  ),
  document.getElementById('app')!,
);

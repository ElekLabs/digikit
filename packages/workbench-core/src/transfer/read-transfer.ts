import { batch } from 'solid-js';
import { isDeviceId, runtimeDeviceIds } from '../data/device-profiles';
import type { TransferApp } from './app';
import { rowKey, storageLine, type TransferView } from './transfer-core';

/** Tick the modules named in the page URL (`?add=`) once the list has them; a row's id is the full module id (`<author>/<name>`). */
function applyPendingAdd(app: TransferApp, view: TransferView): void {
  const { pendingAdd, setTicks, setPendingAdd } = app.state;
  const pending = pendingAdd();
  if (!pending.length) return;
  const keys = view.modules.filter((r) => r.id && pending.some((p) => p === r.id)).map(rowKey);
  setTicks((t) => new Set([...t, ...keys]));
  setPendingAdd([]);
}

/** Read what the mode shows, both sides: the device and the computer (the library, or the chosen samples folder). */
export async function readTransfer(app: TransferApp): Promise<void> {
  const { env } = app;
  const { target } = app.target;
  const { activeMode, browseDevice, browsing, lastView, setBrowsing, setBrowseDevice, setDeviceInfo, setLastView } =
    app.state;
  if (activeMode() === 'samples' && env.extras?.samples) return env.extras.samples.read();
  const readingTarget = target();
  const offline = app.target.libraryOnly();
  const current = () =>
    target() === readingTarget && activeMode() === 'modules' && app.target.libraryOnly() === offline;
  let local: TransferView | undefined;
  const showLocal = (library: TransferView) => {
    app.state.rememberLibrary(library);
    setBrowsing(true);
    setLastView(library);
    setDeviceInfo(undefined);
    applyPendingAdd(app, library);
  };
  const showCard = (view: TransferView) => {
    batch(() => {
      setLastView(view);
      setBrowsing(false);
      applyPendingAdd(app, view);
    });
  };
  if (offline) {
    const library = await app.backend.listLibrary(browseDevice());
    if (current()) showLocal(library);
    return;
  }
  let v: TransferView;
  try {
    v = await app.backend.list(
      browseDevice(),
      (library) => {
        if (!current()) return;
        local = library;
        // Keep the previous device rows during a refresh; only a first read needs the library-only view.
        if (!lastView() || browsing()) showLocal(library);
        else app.state.rememberLibrary(library);
      },
      (view) => {
        if (current() && (!lastView() || browsing())) showCard(view);
      },
    );
  } catch (e) {
    if (current() && local) showLocal(local);
    throw e;
  }
  if (!current()) return;
  setBrowseDevice(isDeviceId(v.device) ? v.device : ((env.libraryDevices ?? runtimeDeviceIds)[0] ?? ''));
  showCard(v);
  const name = await env.deviceName(v.device);
  if (current()) setDeviceInfo({ name, runtime: v.runtime, storage: storageLine(v) });
}

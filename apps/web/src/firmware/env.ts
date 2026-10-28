// What the Firmware page takes from the app it runs in. The page (FirmwarePage.tsx and the files beside it) is the same code in the desktop
// workbench and on the site; the parts that differ are behind this interface. The MIDI side is the Transfer pane's (`TransferEnv.targets`:
// ports, open, close), so the target selector and the connection indicator are the same on every page.
//   desktop: apps/desktop/ui/src/desktop-firmware.ts (patchkit in process through a Tauri command, the system's save dialog)
//   site:    apps/web/src/firmware/web-env.ts (patchkit.wasm in a worker, a download link)
import type { FlashSender, PreparedFlash, RpcMachine } from '../patch/patchkit-core';

/** One run of the page (a flash, a restore, a download), for the app's log. */
export interface FirmwareRun {
  level: 'info' | 'error';
  text: string;
  /** More lines, one each. */
  detail?: string;
}

export interface FirmwareEnv {
  /**
   * Patch the stock file and check the result (`flash::prepare`), or with `restore` check the stock file itself (`flash::restore`). A failed
   * check is in the result; a file that is no known stock image throws patchkit's text.
   */
  prepare(stock: Uint8Array, restore: boolean): Promise<PreparedFlash>;
  /** The Rust sender (`flash::Sender`) for this many messages: the pacing has one source of truth. */
  sender(total: number): Promise<FlashSender>;
  /** The RPC flash machine (`flash_rpc::RpcFlash`) for the image: the one state machine of the default route. `chunk` 0 is the default size. */
  rpc(image: Uint8Array, chunk: number): Promise<RpcMachine>;
  /** Give the user the image as a .syx file. Resolves with where it went (a path, or the file name) and undefined when the user cancelled. */
  saveImage(name: string, bytes: Uint8Array): Promise<string | undefined>;
  /** The site only: open the patched image in the emulator page of this browser (nothing is uploaded). The desktop app has its own emulator. */
  runInEmulator?(name: string, device: string, bytes: Uint8Array): Promise<void>;
  /** The app's log of what happened. */
  log(run: FirmwareRun): void;
}

let current: FirmwareEnv | undefined;

export function setFirmwareEnv(env: FirmwareEnv): void {
  current = env;
}

export function firmwareEnv(): FirmwareEnv {
  if (!current) throw new Error('The Firmware page has no environment: call setFirmwareEnv first.');
  return current;
}

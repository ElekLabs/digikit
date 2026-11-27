// What the Firmware page takes from the app it runs in. The page (FirmwarePage.tsx and the files beside it) is the same code in the desktop
// workbench and on the site; the parts that differ are behind this interface. The MIDI side is the Transfer pane's (`TransferEnv.targets`:
// ports, open, close), so the target selector and the connection indicator are the same on every page.
//   desktop: apps/desktop/ui/src/desktop-firmware.ts (patchkit in process through a Tauri command, the system's save dialog)
//   site:    apps/web/src/firmware/web-env.ts (patchkit.wasm in a worker, a download link)
import type { DkSession } from '@digi/dk-session';
import type { SystemPackage } from '../runtime/activate-system-package';
import type { FlashSender, PreparedFlash, RpcMachine } from '../patch/patchkit-core';
import type { TargetApp } from '../transfer/target';
import type { DeviceState, FlashState } from './firmware-core';
import type { DefaultModuleEntry, DefaultProgress } from './default-modules-core';

/** One run of the page (a flash, a restore, a download), for the app's log. */
export interface FirmwareRun {
  level: 'info' | 'error';
  text: string;
  /** More lines, one each. */
  detail?: string;
}

type Binding = Pick<ReturnType<TargetApp['bindSession']>, 'target' | 'check'> & {
  session(): Promise<Pick<DkSession, 'reserveTransport'>>;
};

/** The desktop controller can verify a guest flash against the prepared container and the new boot. */
export interface EmulatorFlash {
  probe(binding: Binding): Promise<DeviceState>;
  flash(
    binding: Binding,
    p: PreparedFlash,
    signal: AbortSignal,
    report: (state: FlashState) => void,
  ): Promise<{ state: FlashState; beforePackageRestart?: () => Promise<() => Promise<void>> }>;
}

export interface FirmwareEnv {
  emulatorFlash?: EmulatorFlash;
  /**
   * Which devices' patched images this app offers. The public site passes its own list (apps/web/src/firmware/site-policy.ts: a publication decision, not a device
   * fact); the Firmware page then prepares no patched image for another device (restoring stock stays available for every device). The desktop workbench leaves
   * it out and offers every device that has a `device` patch set.
   */
  offersPatched?(device: string): boolean;
  /** The devices the page names and defaults to, in order. The site passes the devices it offers; the desktop workbench leaves it out (every runtime device). */
  devices?: readonly string[];
  /**
   * Patch the stock file and check the result (`flash::prepare`), or with `restore` check the stock file itself (`flash::restore`). A failed
   * check is in the result; a file that is no known stock image throws patchkit's text. `set` is the device file's patch set: empty for `device`,
   * `device-introblank` with the Custom boot screen toggle.
   */
  prepare(stock: Uint8Array, restore: boolean, set?: string): Promise<PreparedFlash>;
  /** This build's system package, read before sending a runtime image. */
  systemPackage?(): Promise<SystemPackage>;
  /** The Rust sender (`flash::Sender`) for this many messages: the pacing has one source of truth. */
  sender(total: number): Promise<FlashSender>;
  /** The RPC flash machine (`flash_rpc::RpcFlash`) for the image: the one state machine of the default route. `chunk` 0 is the default size. */
  rpc(image: Uint8Array, chunk: number): Promise<RpcMachine>;
  /** Give the user the image as a .syx file. Resolves with where it went (a path, or the file name) and undefined when the user cancelled. */
  saveImage(name: string, bytes: Uint8Array): Promise<string | undefined>;
  /** The site only, and only when `SHOW_EMULATOR` in `site.config.mjs` is on: the patched image opens in the emulator page of this browser and nothing is uploaded. The desktop app has its own emulator. */
  runInEmulator?(name: string, device: string, bytes: Uint8Array): Promise<void>;
  /** The app's log of what happened. */
  log(run: FirmwareRun): void;
  /**
   * The desktop app only: modules that are installed and enabled together with the firmware (`[digikit] default_modules` of the device file), on
   * the flashed device after its flash and in the emulator after it loads the patched image. The site leaves it out (it has no module install).
   */
  defaultModules?: {
    /** The list for the device id; the data is the device file's. */
    entries(device: string): DefaultModuleEntry[];
    /** Install and enable `ids` (all ticked) on the selected MIDI device or on the emulator. Throws `DefaultModuleError` for the module that failed. */
    install(
      where: 'device' | 'emulator',
      device: string,
      ids: string[],
      progress: (p: DefaultProgress) => void,
      opts?: { signal?: AbortSignal; stillBooting?: () => boolean },
    ): Promise<string[]>;
  };
}

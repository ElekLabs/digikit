// The desktop workbench's side of the Firmware page (apps/web/src/firmware/env.ts): patchkit in process through the Tauri command
// `firmware_prepare` (the checkout's modules, ports and device files), the Rust sender and the RPC flash machine through patchkit.wasm (the same code the site runs),
// the system's save dialog, and the Log panel. The MIDI port is the Transfer pane's (desktop-env.ts).
import type { TransferApp } from '../../../web/src/transfer/app';
import type { FirmwareEnv } from '../../../web/src/firmware/env';
import { PatchKit, preparedFromSummary, type FlashSummary } from '../../../web/src/patch/patchkit-core';
import { createDesktopDefaultModules } from './default-modules';
import { flashEntry } from './log-core';
import { logInput } from './log-state';
import type { Host } from './host/host';

/** The answer of `firmware_prepare`: 4 bytes of JSON length (big endian), the summary JSON, the image. */
export function unpack(buf: Uint8Array): { summary: FlashSummary; image: Uint8Array } {
  const n = new DataView(buf.buffer, buf.byteOffset, buf.byteLength).getUint32(0);
  return {
    summary: JSON.parse(new TextDecoder().decode(buf.subarray(4, 4 + n))) as FlashSummary,
    image: buf.slice(4 + n),
  };
}

/**
 * The Firmware page's environment over `host`; the patchkit wasm is loaded once (a failed load is tried again at the next use). `app` is the
 * Transfer app whose sessions the default modules install over (a function: the apps are built from the environments).
 */
export function createDesktopFirmwareEnv(host: Host, app: () => TransferApp): FirmwareEnv {
  let sender: Promise<PatchKit> | undefined;
  /** The sender needs no firmware or modules: an empty bundle. */
  const kit = (): Promise<PatchKit> => {
    sender ??= host.firmware
      .patchkitWasm()
      .then((b) => PatchKit.create(b, '{"ports":[],"modules":[]}'))
      .catch((e) => {
        sender = undefined;
        throw e;
      });
    return sender;
  };
  return {
    async prepare(stock, restore, set = '') {
      const raw = new Uint8Array(await host.firmware.prepare(stock, { restore, set }));
      const { summary, image } = unpack(raw);
      return preparedFromSummary(summary, image);
    },
    async sender(total) {
      return (await kit()).sender(total);
    },
    // the same state machine as the site, from the same wasm (the desktop app moves the bytes over midir)
    async rpc(image, chunk) {
      return (await kit()).rpcFlash(image, chunk);
    },
    async saveImage(name, bytes) {
      return (await host.firmware.save(bytes, name)) ?? undefined;
    },
    log: (run) => logInput(flashEntry(run)),
    defaultModules: createDesktopDefaultModules(app),
  };
}

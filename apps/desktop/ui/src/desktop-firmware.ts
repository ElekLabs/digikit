// The desktop workbench's side of the Firmware page (apps/web/src/firmware/env.ts): patchkit in process through the Tauri command
// `firmware_prepare` (the checkout's modules, ports and device files), the Rust sender through patchkit.wasm (the same code the site runs),
// the system's save dialog, and the Log panel. The MIDI port is the Transfer pane's (desktop-env.ts).
import { invoke } from '@tauri-apps/api/core';
import { setFirmwareEnv, type FirmwareEnv } from '../../../web/src/firmware/env';
import { PatchKit, preparedFromSummary, type FlashSummary } from '../../../web/src/patch/patchkit-core';
import { flashEntry } from './log-core';
import { logInput } from './log-state';

let sender: Promise<PatchKit> | undefined;
/** The sender needs no firmware or modules: an empty bundle. */
function kit(): Promise<PatchKit> {
  sender ??= invoke<ArrayBuffer>('patchkit_wasm')
    .then((b) => PatchKit.create(b, '{"ports":[],"modules":[]}'))
    .catch((e) => {
      sender = undefined;
      throw e;
    });
  return sender;
}

/** The answer of `firmware_prepare`: 4 bytes of JSON length (big endian), the summary JSON, the image. */
export function unpack(buf: Uint8Array): { summary: FlashSummary; image: Uint8Array } {
  const n = new DataView(buf.buffer, buf.byteOffset, buf.byteLength).getUint32(0);
  return {
    summary: JSON.parse(new TextDecoder().decode(buf.subarray(4, 4 + n))) as FlashSummary,
    image: buf.slice(4 + n),
  };
}

export const desktopFirmwareEnv: FirmwareEnv = {
  async prepare(stock, restore) {
    const raw = new Uint8Array(
      await invoke<ArrayBuffer>('firmware_prepare', stock, { headers: { 'x-restore': restore ? '1' : '0' } }),
    );
    const { summary, image } = unpack(raw);
    return preparedFromSummary(summary, image);
  },
  async sender(total) {
    return (await kit()).sender(total);
  },
  async saveImage(name, bytes) {
    return (
      (await invoke<string | null>('firmware_save', bytes, { headers: { 'x-name': encodeURIComponent(name) } })) ??
      undefined
    );
  },
  log: (run) => logInput(flashEntry(run)),
};

setFirmwareEnv(desktopFirmwareEnv);

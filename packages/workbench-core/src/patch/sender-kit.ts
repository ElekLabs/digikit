// The Rust sender and RPC flash machine of the Firmware page come from one instance of patchkit.wasm that needs no firmware or modules (an
// empty bundle). `senderKit(wasm)` loads it once from the app's WasmSource; a failed load is tried again at the next use.
import type { WasmSource } from '../wasm-source';
import { PatchKit, type PatchKitLike } from './patchkit-core';

const EMPTY_BUNDLE = '{"ports":[],"modules":[]}';

/** `create` makes the kit from the wasm bytes (a test gives a stub that makes a PatchKitLike without wasm). */
export function senderKit(
  wasm: WasmSource,
  create: (bytes: ArrayBuffer, bundle: string) => Promise<PatchKitLike> = PatchKit.create,
): () => Promise<PatchKitLike> {
  let kit: Promise<PatchKitLike> | undefined;
  return () => {
    kit ??= wasm
      .bytes('patchkit.wasm')
      .then((b) => create(b, EMPTY_BUNDLE))
      .catch((e) => {
        kit = undefined;
        throw e;
      });
    return kit;
  };
}

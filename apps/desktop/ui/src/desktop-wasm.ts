// The desktop app's WasmSource (packages/workbench-core/src/wasm-source.ts): the wasm files come through the Host (the Tauri commands that read the checkout's
// builds), not over HTTP.
import type { WasmSource } from '@digi/workbench-core/wasm-source.ts';
import type { Host } from '@digi/workbench-core/host/host.ts';

export function hostWasmSource(host: Host): WasmSource {
  return {
    bytes(name) {
      switch (name) {
        case 'patchkit.wasm':
          return host.firmware.patchkitWasm();
        case 'dksample.wasm':
          return host.firmware.sampleWasm();
        default:
          return Promise.reject(new Error(`The desktop app has no wasm file ${name}.`));
      }
    },
  };
}

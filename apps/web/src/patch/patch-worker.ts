// Runs patchkit.wasm off the main thread. Messages in: init, check, apply, prepare; out: ready, checked, patched, prepared, error.
// The firmware arrives as an ArrayBuffer and never leaves this page: there is no network call here except
// fetching the (firmware-free) wasm and module bundle that the site itself serves. The wasm comes from a WasmSource over `base` (a worker
// cannot be given the page's objects, so it builds its own from the base URL in `init`).
import { fetchWasmSource } from '@digi/workbench-core/wasm-source.ts';
import { PatchKit } from '@digi/workbench-core/patch/patchkit-core.ts';

export type PatchRequest =
  | { type: 'init'; base: string }
  | { type: 'check'; id: number; firmware: ArrayBuffer }
  | { type: 'apply'; id: number; firmware: ArrayBuffer; modules: string[] }
  | { type: 'prepare'; id: number; firmware: ArrayBuffer; restore: boolean; set?: string };

let kit: Promise<PatchKit> | undefined;
const post = (message: unknown, transfer: Transferable[] = []) =>
  (self as unknown as Worker).postMessage(message, transfer);

self.onmessage = async ({ data }: MessageEvent<PatchRequest>) => {
  try {
    if (data.type === 'init') {
      kit = (async () => {
        const bundle = await fetch(`${data.base}modules/bundle.json`);
        if (!bundle.ok) throw new Error('patch tool files are missing from this site');
        return PatchKit.create(await fetchWasmSource(data.base).bytes('patchkit.wasm'), await bundle.text());
      })();
      await kit;
      post({ type: 'ready' });
      return;
    }
    const k = await kit;
    if (!k) throw new Error('patch worker is not initialised');
    const firmware = new Uint8Array(data.firmware);
    if (data.type === 'prepare') {
      const p = k.prepare(firmware, data.restore, data.set ?? '');
      post({ type: 'prepared', id: data.id, prepared: { ...p, image: p.image.buffer } }, [p.image.buffer]);
    } else if (data.type === 'check') {
      post({ type: 'checked', id: data.id, result: k.check(firmware) });
    } else {
      const r = k.apply(firmware, data.modules);
      post({ type: 'patched', id: data.id, plan: r.plan, verify: r.verify, sha256: r.sha256, syx: r.syx.buffer }, [
        r.syx.buffer,
      ]);
    }
  } catch (error) {
    post({
      type: 'error',
      id: (data as { id?: number }).id,
      error: error instanceof Error ? error.message : String(error),
    });
  }
};

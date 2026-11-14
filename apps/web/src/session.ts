// Starting a browser session from a .syx the visitor picks: hash it, look for
// its ready state in IndexedDB, else (cold boot, see `coldBoot`) build it, then
// run the ColdFire and the DSP in two workers over a SharedArrayBuffer. When the
// page is not cross-origin isolated (coi-serviceworker.js not yet active, or
// unsupported) the caller falls back to the single-thread core in
// runtime-worker.ts. No firmware or derived data ships with the site; the .syx
// and everything made from it stay in the visitor's browser.
import type { BlobStore } from './blob-store.ts';
import { ReadyCache, sha256Hex, type ReadyState } from './ready-cache.ts';
import { createLinkBuffer, LinkServer } from './sab-link.ts';
import type { CfStart } from './cf-worker.ts';
import type { DspStart } from './dsp-worker.ts';
import type { WorkerFactory, WorkerPort } from './workers.ts';

export interface SessionOptions {
  /** The page's workers (`browserWorkers()`; a test gives fakes). */
  workers: WorkerFactory;
  /** Where the ready state is kept (`indexedDbBlobStore('digi-ready-state', 'states')` in the browser). */
  store: BlobStore;
  /** URL of the generic core (`emulator-core-generic.wasm`: no firmware in it). */
  coreUrl: string;
  /** DSP fast tier: threshold, min insns, follow; default 256,64,0 (see 6f819d9^:docs/notes/laneB.md). */
  fast?: [number, number, number];
  /** The audio worklet's port: interleaved L/R f32 blocks arrive here. */
  pcmPort: MessagePort;
  onStatus?: (status: unknown) => void;
}

export interface Session {
  cf: WorkerPort;
  dsp: WorkerPort;
  stop(): void;
  /** Where the ready state came from. */
  source: 'cache' | 'cold-boot';
}

export function canSplit(): boolean {
  return (
    typeof SharedArrayBuffer !== 'undefined' &&
    (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true
  );
}

/**
 * The cold boot: run the ColdFire from the .syx until the firmware is at
 * ready with the DSP attached, and take the three products (packed DSP image
 * from the .syx's SHARC section, DSP state, ColdFire snapshot). NOT BUILT YET:
 * the pieces exist natively (packages/emulator/loader decodes the .syx and is wasm32
 * clean; the removed dt2-native-host crate booted a .syx to ready natively), not in the browser core.
 */
export async function coldBoot(_syx: Uint8Array, _progress?: (fraction: number) => void): Promise<ReadyState> {
  throw new Error('cold boot from a .syx is not built in the browser core yet (see 6f819d9^:docs/notes/laneB.md)');
}

export async function startSession(syx: Uint8Array, o: SessionOptions): Promise<Session> {
  if (!canSplit()) throw new Error('not cross-origin isolated: use the single-thread core');
  const cache = new ReadyCache(o.store);
  const key = await sha256Hex(syx as BufferSource);
  let state = await cache.load(key);
  const source = state ? 'cache' : 'cold-boot';
  if (!state) {
    state = await coldBoot(syx);
    void cache.save(key, state);
  }
  const link = createLinkBuffer();
  const dsp = o.workers.create('dsp');
  const cf = o.workers.create('cf');
  const ready = (w: WorkerPort) =>
    new Promise<void>((resolve, reject) => {
      w.onmessage = ({ data }) =>
        data.type === 'ready' ? resolve() : data.type === 'error' ? reject(new Error(data.error)) : o.onStatus?.(data);
      w.onerror = (e) => reject(new Error(e.message));
    });
  const dspReady = ready(dsp);
  const start: DspStart = {
    type: 'start',
    coreUrl: o.coreUrl,
    image: state.image,
    state: state.dsp,
    link,
    fast: o.fast ?? [256, 64, 0],
    pcmPort: o.pcmPort,
  };
  dsp.postMessage(start, [o.pcmPort]);
  await dspReady; // the DSP must answer before the ColdFire sends its first frame
  const cfReady = ready(cf);
  const cfStart: CfStart = { type: 'start', coreUrl: o.coreUrl, syx, snapshot: state.snapshot, link };
  cf.postMessage(cfStart);
  await cfReady;
  return {
    cf,
    dsp,
    source,
    stop() {
      new LinkServer(link).close();
      cf.terminate();
      dsp.terminate();
    },
  };
}

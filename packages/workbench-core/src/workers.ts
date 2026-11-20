// The Web Workers of the apps, behind a factory: code that needs a worker takes a `WorkerFactory` and asks for a kind; the site's entry builds
// `browserWorkers()` (apps/web/src/browser-workers.ts), a test gives `createFakeWorkers()` (workers-fake.ts). This file has the interfaces only.

/** The part of a Worker that the app uses: messages in and out, errors, and the end. */
export interface WorkerPort {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: { message: string }) => void) | null;
  addEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
  removeEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
  terminate(): void;
}

/** `runtime`: the single-worker emulator (runtime/runtime-worker.ts). `dsp` and `cf`: the two halves of the split session (dsp-worker.ts, cf-worker.ts). `patch`: patchkit.wasm off the main thread (patch/patch-worker.ts). */
export type WorkerKind = 'runtime' | 'dsp' | 'cf' | 'patch';

export interface WorkerFactory {
  create(kind: WorkerKind): WorkerPort;
}

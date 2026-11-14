// The Web Workers of the site, behind a factory: code that needs a worker takes a `WorkerFactory` and asks for a kind; the page's entry builds
// `browserWorkers()`, a test gives `createFakeWorkers()` (workers-fake.ts). This is the only file that calls `new Worker` (oxlint
// `no-restricted-globals`). The `new Worker(new URL(...))` calls stay literal here, one per kind, so the bundler finds each worker file.

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

/** The browser's workers. */
export function browserWorkers(): WorkerFactory {
  const module = { type: 'module' } as const;
  return {
    create(kind) {
      switch (kind) {
        case 'runtime':
          return new Worker(new URL('./runtime/runtime-worker.ts', import.meta.url), module) as unknown as WorkerPort;
        case 'dsp':
          return new Worker(new URL('./dsp-worker.ts', import.meta.url), module) as unknown as WorkerPort;
        case 'cf':
          return new Worker(new URL('./cf-worker.ts', import.meta.url), module) as unknown as WorkerPort;
        case 'patch':
          return new Worker(new URL('./patch/patch-worker.ts', import.meta.url), module) as unknown as WorkerPort;
      }
    },
  };
}

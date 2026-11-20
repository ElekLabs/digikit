// The browser's workers: the only file that calls `new Worker` (oxlint `no-restricted-globals`). The `new Worker(new URL(...))` calls stay literal
// here, one per kind, so the bundler finds each worker file. The interfaces and the fake are in @digi/workbench-core (workers.ts, workers-fake.ts).
import type { WorkerFactory, WorkerPort } from '@digi/workbench-core/workers.ts';

/** The browser's workers. */
export function browserWorkers(): WorkerFactory {
  const module = { type: 'module' } as const;
  return {
    create(kind) {
      switch (kind) {
        case 'runtime':
          return new Worker(new URL('./runtime/runtime-worker.ts', import.meta.url), module) as unknown as WorkerPort;
        case 'dsp':
          return new Worker(
            new URL('../../../packages/workbench-core/src/dsp-worker.ts', import.meta.url),
            module,
          ) as unknown as WorkerPort;
        case 'cf':
          return new Worker(
            new URL('../../../packages/workbench-core/src/cf-worker.ts', import.meta.url),
            module,
          ) as unknown as WorkerPort;
        case 'patch':
          return new Worker(new URL('./patch/patch-worker.ts', import.meta.url), module) as unknown as WorkerPort;
      }
    },
  };
}

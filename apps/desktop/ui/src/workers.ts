import type { WorkerFactory, WorkerPort } from '@digi/workbench-core/workers.ts';

export function createAnalyzerWorkers(): WorkerFactory<'analyzer'> {
  return {
    create: () =>
      new Worker(new URL('./analyzer-worker.ts', import.meta.url), { type: 'module' }) as unknown as WorkerPort,
  };
}

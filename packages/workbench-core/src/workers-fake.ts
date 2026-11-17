// In-process workers for tests: `createFakeWorkers(handlers)` is a WorkerFactory whose workers run a function of the test instead of a thread.
// A message posted to a worker goes to the handler of its kind (in a microtask, as a real worker answers later); the handler answers with
// `reply(data)`, which reaches the page's `onmessage` and listeners like a worker's message. `terminate` ends the channel.
import type { WorkerFactory, WorkerKind, WorkerPort } from './workers';

export interface FakeWorker extends WorkerPort {
  readonly kind: WorkerKind;
  /** Every message posted to the worker, in order. */
  readonly posted: { message: unknown; transfer: Transferable[] }[];
  terminated: boolean;
  /** The worker sends `data` to the page. */
  reply(data: unknown): void;
  /** The worker fails to start or crashes. */
  fail(message: string): void;
}

export type FakeHandler = (message: unknown, worker: FakeWorker) => void | Promise<void>;

export interface FakeWorkers extends WorkerFactory {
  /** The workers made so far, in order. */
  readonly made: FakeWorker[];
}

export function createFakeWorkers(handlers: Partial<Record<WorkerKind, FakeHandler>> = {}): FakeWorkers {
  const made: FakeWorker[] = [];
  return {
    made,
    create(kind) {
      const listeners = new Set<(event: MessageEvent) => void>();
      const worker: FakeWorker = {
        kind,
        posted: [],
        terminated: false,
        onmessage: null,
        onerror: null,
        postMessage(message, transfer = []) {
          if (worker.terminated) return;
          worker.posted.push({ message, transfer });
          queueMicrotask(() => {
            if (!worker.terminated) void handlers[kind]?.(message, worker);
          });
        },
        addEventListener: (_type, listener) => void listeners.add(listener),
        removeEventListener: (_type, listener) => void listeners.delete(listener),
        terminate() {
          worker.terminated = true;
        },
        reply(data) {
          if (worker.terminated) return;
          const event = { data } as MessageEvent;
          worker.onmessage?.(event);
          for (const listener of listeners) listener(event);
        },
        fail(message) {
          worker.onerror?.({ message });
        },
      };
      made.push(worker);
      return worker;
    },
  };
}

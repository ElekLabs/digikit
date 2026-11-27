// `createEffect(on(source, run, { defer: true }))`: run when the source changes, not at the first read.
import { createEffect, on } from 'solid-js';

export function createEffectOn<T>(source: () => T, run: (value: T) => void): void {
  createEffect(on(source, run, { defer: true }));
}

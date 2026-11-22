import { createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { readDeviceHealth, type DeviceHealth } from './device-health';
import { windowShown } from './log-follow-core';

export type ReceivedHealth = DeviceHealth & { received: number; source: TargetBinding };

export function createTargetHealth(binding: () => TargetBinding, enabled: () => boolean) {
  const [sample, setSample] = createSignal<ReceivedHealth>();
  const active = createMemo(enabled);
  createEffect(() => {
    const source = binding();
    setSample(undefined);
    if (source.target.kind !== 'midi' || !active()) return;
    const stop = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        if (windowShown()) {
          const session = await source.session();
          const health = await readDeviceHealth(session, stop.signal);
          if (!stop.signal.aborted)
            setSample(
              (prev) =>
                health &&
                (prev?.seq === health.seq && prev.blocks === health.blocks
                  ? prev
                  : { ...health, source, received: Date.now() }),
            );
        }
      } catch {
        if (!stop.signal.aborted) setSample(undefined);
      } finally {
        if (!stop.signal.aborted) timer = setTimeout(() => void poll(), 5000);
      }
    };
    untrack(() => void poll());
    onCleanup(() => {
      stop.abort();
      clearTimeout(timer);
    });
  });
  return sample;
}

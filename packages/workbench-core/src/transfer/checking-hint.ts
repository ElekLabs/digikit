// Brief per-file checks stay quiet; a slow check explains why a full byte bar is still active.
import { createEffect, createMemo, createSignal, onCleanup, type Accessor } from 'solid-js';

/** A file path identifies one check. Replacing the surrounding progress object must not restart its delay. */
export function createCheckingHint(getFile: () => string | undefined): Accessor<boolean> {
  const file = createMemo(getFile);
  const [shown, setShown] = createSignal(false);
  createEffect(() => {
    setShown(false);
    if (!file()) return;
    const timer = setTimeout(() => setShown(true), 1000);
    onCleanup(() => clearTimeout(timer));
  });
  return shown;
}

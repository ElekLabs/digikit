import { createEffect, createMemo, createSignal, onCleanup } from 'solid-js';
import type { PerformanceCapture } from '@digi/workbench-core/profile/performance-capture.ts';
import { readSavedProfile, type SavedEvidence, type SavedProfile } from './hotspots';

/** Resolves the selected capture's symbols while the Profile view is visible. */
export function createProfileReference(deps: {
  capture(): PerformanceCapture | undefined;
  visible(): boolean;
  resolve(profile: SavedProfile): Promise<SavedEvidence>;
}) {
  const profile = createMemo(() => {
    const capture = deps.capture();
    return capture && readSavedProfile(capture.document);
  });
  const [evidence, setEvidence] = createSignal<SavedEvidence>();

  createEffect(() => {
    const saved = profile();
    setEvidence(saved && { pcs: saved.pcs });
    if (!saved || !deps.visible() || !(saved.pcs.cf.length || saved.pcs.sharc.length)) return;
    let current = true;
    onCleanup(() => (current = false));
    void deps.resolve(saved).then((resolved) => {
      if (current) setEvidence(resolved);
    });
  });

  return { evidence };
}

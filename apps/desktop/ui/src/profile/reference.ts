import { batch, createEffect, createSignal, onCleanup } from 'solid-js';
import { readSavedProfile, type SavedProfile } from './hotspots';

type ProfileFile = Pick<File, 'name' | 'text'>;

/** Keeps a saved comparison under the Solid owner of one target binding. */
export function createProfileReference(deps: {
  visible(): boolean;
  resolve(profile: SavedProfile): Promise<SavedProfile['pcs']>;
}) {
  const [selection, setSelection] = createSignal<{ name: string; profile: SavedProfile }>();
  const [error, setError] = createSignal('');
  const [pcs, setPcs] = createSignal<SavedProfile['pcs']>();
  let opening = 0;
  let disposed = false;
  onCleanup(() => (disposed = true));

  createEffect(() => {
    const saved = selection();
    setPcs(saved?.profile.pcs);
    if (!saved || !deps.visible()) return;
    let current = true;
    onCleanup(() => (current = false));
    void deps.resolve(saved.profile).then((resolved) => {
      if (current && !disposed) setPcs(resolved);
    });
  });

  const open = async (file: ProfileFile | undefined) => {
    if (!file || disposed) return;
    const request = ++opening;
    const current = () => request === opening && !disposed;
    try {
      const text = await file.text();
      if (!current()) return;
      const profile = readSavedProfile(JSON.parse(text));
      if (!profile.shares && !profile.pcs.cf.length && !profile.pcs.sharc.length) {
        setError(`${file.name} has no span window or sampled PCs.`);
        return;
      }
      batch(() => {
        setSelection({ name: file.name, profile });
        setError('');
      });
    } catch (e) {
      if (current()) setError(`${file.name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return { selection, pcs, error, open };
}

import { createSignal } from 'solid-js';
import type { KeyValue } from '@digi/workbench-core/storage.ts';

export const themes = { digikit: 'Digikit', linear: 'Violet Slate' } as const;
type Theme = keyof typeof themes;

export function createTheme(storage: KeyValue, root: HTMLElement) {
  const key = 'digikit.appearance.theme';
  const saved = storage.get(key);
  const [current, setCurrent] = createSignal<Theme>(saved === 'linear' ? saved : 'digikit');
  root.dataset.theme = current();
  return {
    current,
    select(value: string) {
      if (value !== 'linear' && value !== 'digikit') return;
      root.dataset.theme = value;
      setCurrent(value);
      storage.set(key, value);
    },
  };
}

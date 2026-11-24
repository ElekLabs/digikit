import { batch, createSignal, onCleanup } from 'solid-js';
import { readPerformanceCaptures } from '@digi/workbench-core/profile/performance-capture.ts';

export function createSavedPerformance() {
  const [file, setFile] = createSignal<ReturnType<typeof readPerformanceCaptures>>();
  const [index, setIndex] = createSignal(0);
  const [error, setError] = createSignal('');
  let revision = 0;
  onCleanup(() => revision++);

  return {
    file,
    index,
    error,
    current: () => file()?.[index()],
    select(value: number) {
      const capture = file()?.[value];
      if (capture) setIndex(value);
      return capture;
    },
    clear() {
      revision++;
      batch(() => {
        setFile(undefined);
        setError('');
      });
    },
    async open(input: Pick<File, 'name' | 'text'> | undefined) {
      if (!input) return;
      const request = ++revision;
      try {
        const captures = readPerformanceCaptures(JSON.parse(await input.text()), input.name);
        if (request !== revision) return;
        batch(() => {
          setIndex(0);
          setFile(captures);
          setError('');
        });
        return captures[0];
      } catch (cause) {
        if (request === revision)
          setError(input.name + ' · ' + (cause instanceof Error ? cause.message : String(cause)));
      }
    },
  };
}

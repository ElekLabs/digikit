import { createSignal, onCleanup } from 'solid-js';
import { readSavedTraces } from '@digi/workbench-core/profile/trace.ts';

export function createSavedTimeline() {
  const [file, setFile] = createSignal<ReturnType<typeof readSavedTraces>>();
  const [index, setIndex] = createSignal(0);
  const [error, setError] = createSignal('');
  let revision = 0;
  onCleanup(() => revision++);

  return {
    file,
    index,
    error,
    current: () => file()?.[index()],
    select: (value: number) => setIndex(value),
    clear() {
      revision++;
      setFile(undefined);
      setError('');
    },
    async open(input: Pick<File, 'name' | 'text'> | undefined) {
      if (!input) return;
      const request = ++revision;
      try {
        const traces = readSavedTraces(JSON.parse(await input.text()), input.name);
        if (request !== revision) return;
        setIndex(0);
        setFile(traces);
        setError('');
      } catch (cause) {
        if (request === revision)
          setError(input.name + ' · ' + (cause instanceof Error ? cause.message : String(cause)));
      }
    },
  };
}

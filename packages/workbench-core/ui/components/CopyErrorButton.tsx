import { createSignal, onCleanup, type JSX } from 'solid-js';

export default function CopyErrorButton(props: { text: string; class?: string }): JSX.Element {
  const [feedback, setFeedback] = createSignal<{ text: string; message: string }>();
  const visibleFeedback = () => {
    const current = feedback();
    return current?.text === props.text ? current.message : undefined;
  };
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  onCleanup(() => {
    disposed = true;
    clearTimeout(timer);
  });

  const copy = async () => {
    const text = props.text;
    const request = ++attempt;
    let message: string;
    try {
      await navigator.clipboard.writeText(text);
      message = 'Copied';
    } catch {
      message = 'Copy failed';
    }
    if (disposed || request !== attempt || text !== props.text) return;
    clearTimeout(timer);
    setFeedback({ text, message });
    timer = setTimeout(() => setFeedback(undefined), 1500);
  };

  return (
    <button type="button" class={props.class ?? 'act small'} aria-live="polite" onClick={() => void copy()}>
      {visibleFeedback() ?? 'Copy error'}
    </button>
  );
}

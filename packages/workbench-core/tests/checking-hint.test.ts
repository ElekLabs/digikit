import assert from 'node:assert/strict';
import { createRoot, createSignal } from 'solid-js';
import { test, vi } from 'vitest';
import { createCheckingHint } from '../src/transfer/checking-hint.ts';

test('brief checks stay quiet; a slow check survives progress updates and resets for the next file', () => {
  vi.useFakeTimers();
  let dispose = () => {};
  try {
    const [bar, setBar] = createSignal({ checking: '' });
    const shown = createRoot((d) => {
      dispose = d;
      return createCheckingHint(() => bar().checking);
    });
    setBar({ checking: 'Drums/kick' });
    vi.advanceTimersByTime(200);
    assert.equal(shown(), false);
    setBar({ checking: '' });
    vi.advanceTimersByTime(1000);
    assert.equal(shown(), false, 'a finished check cannot show its hint later');

    setBar({ checking: 'Drums/snare' });
    vi.advanceTimersByTime(800);
    setBar({ checking: 'Drums/snare' });
    vi.advanceTimersByTime(200);
    assert.equal(shown(), true, 'a new progress object must not restart the same check');

    setBar({ checking: 'Other/snare' });
    assert.equal(shown(), false, 'the same filename in another folder is a new check');
    vi.advanceTimersByTime(1000);
    assert.equal(shown(), true);
    setBar({ checking: '' });
    assert.equal(shown(), false, 'success, failure or cancellation clears the hint immediately');

    setBar({ checking: 'Other/hat' });
    dispose();
    vi.advanceTimersByTime(1000);
    assert.equal(shown(), false, 'unmounting clears the pending hint');
  } finally {
    dispose();
    vi.useRealTimers();
  }
});

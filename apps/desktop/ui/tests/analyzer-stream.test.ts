import { afterEach, describe, expect, it, vi } from 'vitest';
import { createAnalyzerStream } from '../src/analyzer-stream';

const PCM = { generation: 1, rate: 48000, left: [0], right: [0] };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

afterEach(() => vi.useRealTimers());

describe('analyzer capture', () => {
  it('closes an obsolete open before opening the most recent source', async () => {
    vi.useFakeTimers();
    const slow = deferred<number>();
    const open = vi.fn().mockReturnValueOnce(slow.promise).mockResolvedValueOnce(2);
    const read = vi.fn().mockResolvedValue(PCM);
    const close = vi.fn().mockResolvedValue(undefined);
    const receive = vi.fn();
    const stream = createAnalyzerStream({ open, read, close }, receive, () => true, vi.fn());

    stream.select('remote');
    stream.select('remote');
    const revision = stream.select('emulator');
    expect(open).toHaveBeenCalledTimes(1);
    slow.resolve(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(close).toHaveBeenCalledExactlyOnceWith(1);
    expect(open.mock.invocationCallOrder[1]).toBeGreaterThan(close.mock.invocationCallOrder[0]);
    expect(open).toHaveBeenLastCalledWith('emulator');
    expect(read).toHaveBeenCalledExactlyOnceWith(2);
    expect(receive).toHaveBeenCalledExactlyOnceWith(revision, PCM);
    stream.select(null);
    await vi.advanceTimersByTimeAsync(0);
    expect(close).toHaveBeenLastCalledWith(2);
  });

  it('drops a read that completes after unmount and releases the capture', async () => {
    vi.useFakeTimers();
    const slow = deferred<typeof PCM>();
    const read = vi.fn().mockReturnValue(slow.promise);
    const close = vi.fn().mockResolvedValue(undefined);
    const receive = vi.fn();
    const stream = createAnalyzerStream({ open: async () => 1, read, close }, receive, () => true, vi.fn());
    stream.select('emulator');
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledOnce();
    stream.select(null);
    slow.resolve(PCM);
    await vi.advanceTimersByTimeAsync(200);
    expect(close).toHaveBeenCalledExactlyOnceWith(1);
    expect(receive).not.toHaveBeenCalled();
    expect(read).toHaveBeenCalledOnce();
  });

  it('bounds backpressure and distinguishes unavailable PCM from a silent block', async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockResolvedValue(null);
    const receive = vi.fn();
    const report = vi.fn();
    let ready = false;
    const stream = createAnalyzerStream(
      { open: async () => 1, read, close: async () => {} },
      receive,
      () => ready,
      report,
    );
    const revision = stream.select('remote');
    await vi.advanceTimersByTimeAsync(800);
    expect(read).not.toHaveBeenCalled();
    ready = true;
    await vi.advanceTimersByTimeAsync(80);
    expect(report).toHaveBeenLastCalledWith({ kind: 'waiting' });
    read.mockResolvedValueOnce(PCM);
    await vi.advanceTimersByTimeAsync(40);
    expect(receive).toHaveBeenCalledExactlyOnceWith(revision, PCM);
    expect(report).toHaveBeenLastCalledWith({ kind: 'live' });
    stream.select(null);
  });
});

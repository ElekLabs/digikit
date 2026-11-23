import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AnalyzerPcm } from '@digi/workbench-core/host/host.ts';
import { createAnalyzerStream } from '../src/analyzer-stream';
import { createTauriHost } from '../src/host/tauri-host';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke, isTauri: () => true }));

const PCM: AnalyzerPcm = { generation: 3, rate: 48000, left: [0.25, -0.5], right: [0.5, -0.25] };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

beforeEach(() => {
  invoke.mockReset();
});
afterEach(() => vi.useRealTimers());

describe('Tauri Analyzer adapter', () => {
  it('waits for a pending read and close before reopening', async () => {
    vi.useFakeTimers();
    const pendingRead = deferred<AnalyzerPcm>();
    const pendingClose = deferred<void>();
    invoke
      .mockResolvedValueOnce(17)
      .mockReturnValueOnce(pendingRead.promise)
      .mockReturnValueOnce(pendingClose.promise)
      .mockResolvedValueOnce(18)
      .mockResolvedValueOnce(null);
    const analyzer = createTauriHost().analyzer!;
    const id = await analyzer.open('emulator');
    expect(id).toBe(17);

    const read = analyzer.read(id);
    const close = analyzer.close(id);
    const reopen = analyzer.open('remote');
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke.mock.calls).toEqual([
      ['analyzer_open', { source: 'emulator' }],
      ['analyzer_read', { id: 17 }],
    ]);

    pendingRead.resolve(PCM);
    await expect(read).resolves.toEqual({ generation: 3, rate: 48000, left: [0.25, -0.5], right: [0.5, -0.25] });
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke).toHaveBeenCalledTimes(3);
    expect(invoke).toHaveBeenLastCalledWith('analyzer_close', { id: 17 });

    pendingClose.resolve(undefined);
    await close;
    const newId = await reopen;
    expect(newId).toBe(18);
    expect(invoke).toHaveBeenNthCalledWith(4, 'analyzer_open', { source: 'remote' });
    await expect(analyzer.read(newId)).resolves.toBeNull();
    expect(invoke).toHaveBeenNthCalledWith(5, 'analyzer_read', { id: 18 });
  });

  it('passes a rejected IPC call to its caller and runs the command queued behind it', async () => {
    const failure = new Error('audio source disconnected');
    invoke.mockResolvedValueOnce(17).mockRejectedValueOnce(failure).mockResolvedValueOnce(undefined);
    const analyzer = createTauriHost().analyzer!;
    const id = await analyzer.open('remote');

    const read = analyzer.read(id);
    const close = analyzer.close(id);
    await Promise.all([expect(read).rejects.toBe(failure), expect(close).resolves.toBeUndefined()]);
    expect(invoke.mock.calls).toEqual([
      ['analyzer_open', { source: 'remote' }],
      ['analyzer_read', { id: 17 }],
      ['analyzer_close', { id: 17 }],
    ]);
  });

  it('discards a disposed stream’s in-flight PCM and delivers PCM to the replacement stream', async () => {
    vi.useFakeTimers();
    const pendingRead = deferred<AnalyzerPcm>();
    const replacementPcm = { generation: 4, rate: 44100, left: [0.125], right: [-0.125] };
    invoke.mockImplementation((command, args) => {
      switch (command) {
        case 'analyzer_open':
          return Promise.resolve(args.source === 'emulator' ? 17 : 18);
        case 'analyzer_read':
          return args.id === 17 ? pendingRead.promise : Promise.resolve(replacementPcm);
        case 'analyzer_close':
          return Promise.resolve();
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
    const analyzer = createTauriHost().analyzer!;
    const receiveOld = vi.fn();
    const receiveNew = vi.fn();
    const oldStream = createAnalyzerStream(analyzer, receiveOld, () => true, vi.fn());
    const newStream = createAnalyzerStream(analyzer, receiveNew, () => true, vi.fn());

    oldStream.select('emulator');
    await vi.advanceTimersByTimeAsync(0);
    oldStream.select(null);
    const revision = newStream.select('remote');
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke.mock.calls).toEqual([
      ['analyzer_open', { source: 'emulator' }],
      ['analyzer_read', { id: 17 }],
    ]);

    pendingRead.resolve(PCM);
    await vi.advanceTimersByTimeAsync(0);
    expect(receiveOld).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith('analyzer_close', { id: 17 });
    expect(invoke).toHaveBeenCalledWith('analyzer_open', { source: 'remote' });
    expect(invoke).toHaveBeenCalledWith('analyzer_read', { id: 18 });
    expect(receiveNew).toHaveBeenCalledExactlyOnceWith(revision, replacementPcm);

    newStream.select(null);
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke).toHaveBeenLastCalledWith('analyzer_close', { id: 18 });
  });
});

it('sends log bytes as the native request body with the encoded filename header', async () => {
  const bytes = new Uint8Array([0x00, 0x80, 0xff]);
  invoke.mockResolvedValueOnce(null);

  await expect(createTauriHost().fs.saveLog(bytes, 'Capture #1 – café.log')).resolves.toBeNull();
  expect(invoke).toHaveBeenCalledExactlyOnceWith('log_save', bytes, {
    headers: { 'x-name': 'Capture%20%231%20%E2%80%93%20caf%C3%A9.log', 'x-kind': 'log' },
  });
});

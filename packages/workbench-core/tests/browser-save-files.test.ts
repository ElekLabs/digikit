import { expect, test } from 'vitest';
import {
  browserSaveDeps,
  createBrowserSaveFiles,
  PartialSaveError,
  type SaveDirectory,
} from '../src/host/browser-save-files.ts';

const files = [
  { name: 'a.bin', bytes: Uint8Array.of(1) },
  { name: 'b.bin', bytes: Uint8Array.of(2, 3) },
];

function directory(log: string[]): SaveDirectory {
  return {
    name: 'Samples',
    getFileHandle: async (name) => ({
      createWritable: async () => ({
        write: async (data) => void log.push(`write ${name} ${data.length}`),
        close: async () => void log.push(`close ${name}`),
        abort: async () => void log.push(`abort ${name}`),
      }),
    }),
  };
}

test('with a directory picker the files go into the chosen folder in order', async () => {
  const log: string[] = [];
  const save = createBrowserSaveFiles({
    pickDirectory: async () => directory(log),
    download: () => log.push('download'),
  });
  expect(await save(files)).toEqual({ saved: 2, cancelled: false, where: 'Samples' });
  expect(log).toEqual(['write a.bin 1', 'close a.bin', 'write b.bin 2', 'close b.bin']);
});

test('a cancelled picker writes nothing; another failure is passed on', async () => {
  const log: string[] = [];
  const abort = createBrowserSaveFiles({
    pickDirectory: async () => Promise.reject(new DOMException('no', 'AbortError')),
    download: () => log.push('download'),
  });
  expect(await abort(files)).toEqual({ saved: 0, cancelled: true });
  const broken = createBrowserSaveFiles({
    pickDirectory: async () => Promise.reject(new Error('denied')),
    download: () => {},
  });
  await expect(broken(files)).rejects.toThrow('denied');
  expect(log).toEqual([]);
});

test('without a picker each file is one download, and a bad name starts none', async () => {
  const log: string[] = [];
  const save = createBrowserSaveFiles({ download: (name) => void log.push(name) });
  expect(await save(files)).toEqual({ saved: 2, cancelled: false });
  expect(log).toEqual(['a.bin', 'b.bin']);
  await expect(save([...files, { name: 'x/y', bytes: new Uint8Array() }])).rejects.toThrow(/plain file name/);
  expect(log).toHaveLength(2);
});

test('a failed write aborts its writable and reports how many files were complete', async () => {
  const log: string[] = [];
  const dir = directory(log);
  const open = dir.getFileHandle;
  dir.getFileHandle = async (name, options) => {
    const handle = await open(name, options);
    if (name !== 'b.bin') return handle;
    return {
      createWritable: async () => ({
        ...(await handle.createWritable()),
        write: async () => Promise.reject(new Error('disk full')),
      }),
    };
  };
  const save = createBrowserSaveFiles({ pickDirectory: async () => dir, download: () => {} });
  const err = await save(files).catch((e: unknown) => e);
  expect(err).toBeInstanceOf(PartialSaveError);
  expect(err).toMatchObject({ saved: 1, total: 2 });
  expect((err as Error).message).toMatch(/Saved 1 of 2.*disk full/);
  expect(log).toEqual(['write a.bin 1', 'close a.bin', 'abort b.bin']);
});

test('a download URL is revoked only after a delay that lets the download start', () => {
  const timers: { ms: number; run: () => void }[] = [];
  const revoked: string[] = [];
  const win = {
    URL: { createObjectURL: () => 'blob:x', revokeObjectURL: (u: string) => void revoked.push(u) },
    document: { createElement: () => ({ click: () => {} }) },
    setTimeout: (run: () => void, ms: number) => void timers.push({ ms, run }),
  } as unknown as Window & typeof globalThis;
  browserSaveDeps(win).download('a.bin', Uint8Array.of(1));
  expect(revoked).toEqual([]);
  expect(timers).toHaveLength(1);
  expect(timers[0]!.ms).toBeGreaterThanOrEqual(30_000);
  timers[0]!.run();
  expect(revoked).toEqual(['blob:x']);
});

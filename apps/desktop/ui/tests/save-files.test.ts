import { expect, test } from 'vitest';
import { createSaveFiles, packFiles } from '../src/host/save-files.ts';

const files = [
  { name: 'a.wav', bytes: Uint8Array.of(1, 2, 3) },
  { name: 'é.txt', bytes: Uint8Array.of(9) },
];

test('the packed list in order goes to the one native call, which returns the result', async () => {
  let packed: Uint8Array = new Uint8Array();
  const save = createSaveFiles({
    saveWithDialog: async (bytes) => ((packed = bytes), { saved: 2, cancelled: false, where: '/out' }),
  });
  expect(await save(files)).toEqual({ saved: 2, cancelled: false, where: '/out' });
  expect(packed).toEqual(packFiles(files));
  const view = new DataView(packed.buffer as ArrayBuffer);
  expect(view.getUint32(0)).toBe(5);
  expect(new TextDecoder().decode(packed.subarray(4, 9))).toBe('a.wav');
  expect(view.getUint32(9)).toBe(3);
  expect([...packed.subarray(13, 16)]).toEqual([1, 2, 3]);
});

test('a bad name or a repeat in another case never reaches the native call', async () => {
  let called = false;
  const save = createSaveFiles({ saveWithDialog: async () => ((called = true), { saved: 0, cancelled: true }) });
  await expect(save([{ name: '../x', bytes: new Uint8Array() }])).rejects.toThrow(/plain file name/);
  await expect(
    save([
      { name: 'a', bytes: new Uint8Array() },
      { name: 'A', bytes: new Uint8Array() },
    ]),
  ).rejects.toThrow(/twice/);
  expect(called).toBe(false);
});

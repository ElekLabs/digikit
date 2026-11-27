import { expect, test } from 'vitest';
import { createSaveFiles, packFiles } from '../src/host/save-files.ts';

const files = [
  { name: 'a.wav', bytes: Uint8Array.of(1, 2, 3) },
  { name: 'é.txt', bytes: Uint8Array.of(9) },
];

test('one folder dialog, then the packed list in order', async () => {
  const calls: string[] = [];
  let packed: Uint8Array = new Uint8Array();
  const save = createSaveFiles({
    pickFolder: async () => (calls.push('pick'), '/out'),
    write: async (folder, bytes) => (calls.push(`write ${folder}`), (packed = bytes), 2),
  });
  expect(await save(files)).toEqual({ saved: 2, cancelled: false, where: '/out' });
  expect(calls).toEqual(['pick', 'write /out']);
  expect(packed).toEqual(packFiles(files));
  const view = new DataView(packed.buffer as ArrayBuffer);
  expect(view.getUint32(0)).toBe(5);
  expect(new TextDecoder().decode(packed.subarray(4, 9))).toBe('a.wav');
  expect(view.getUint32(9)).toBe(3);
  expect([...packed.subarray(13, 16)]).toEqual([1, 2, 3]);
});

test('a cancelled dialog writes nothing and a bad name never reaches the dialog', async () => {
  let wrote = false;
  let picked = false;
  const save = createSaveFiles({
    pickFolder: async () => ((picked = true), null),
    write: async () => ((wrote = true), 0),
  });
  expect(await save(files)).toEqual({ saved: 0, cancelled: true });
  expect(wrote).toBe(false);
  picked = false;
  await expect(save([{ name: '../x', bytes: new Uint8Array() }])).rejects.toThrow(/plain file name/);
  expect(picked).toBe(false);
});

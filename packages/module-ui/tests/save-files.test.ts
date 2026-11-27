import { describe, expect, it } from 'vitest';
import { checkSaveFiles, createMemoryModuleChannel, createModuleClient, SaveFilesError } from '../src/index.ts';
import { schema } from './schema.ts';

describe('save files', () => {
  it('accepts plain names and refuses separators, dot names, empty and repeated names', () => {
    const files = (...names: string[]) => names.map((name) => ({ name, bytes: new Uint8Array() }));
    expect(() => checkSaveFiles(files('a.wav', 'b c.txt'))).not.toThrow();
    for (const bad of ['', '..', '.', 'a/b', 'a\\b', '../x', 'a\0b'])
      expect(() => checkSaveFiles(files(bad)), bad).toThrow(SaveFilesError);
    expect(() => checkSaveFiles(files('a', 'a'))).toThrow(/twice/);
  });

  it('sends the files of the page to the channel, one or several', async () => {
    const { channel, inspect } = createMemoryModuleChannel(schema);
    const client = createModuleClient(channel);
    const bytes = new Uint8Array([1, 2]);
    expect(await client.saveFile('one.bin', bytes)).toMatchObject({ saved: 1, cancelled: false });
    bytes[0] = 9;
    await client.saveFiles([
      { name: 'a', bytes },
      { name: 'b', bytes },
    ]);
    expect(inspect.saved.map((s) => s.map((f) => f.name))).toEqual([['one.bin'], ['a', 'b']]);
    expect([...inspect.saved[0]![0]!.bytes]).toEqual([1, 2]);
  });

  it('does not reach the channel with a bad name', async () => {
    const { channel, inspect } = createMemoryModuleChannel(schema);
    await expect(createModuleClient(channel).saveFile('../x', new Uint8Array())).rejects.toThrow(SaveFilesError);
    expect(inspect.saved).toEqual([]);
  });
});

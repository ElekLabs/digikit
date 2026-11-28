import { describe, expect, it } from 'vitest';
import { checkSaveFiles, createMemoryModuleChannel, createModuleClient, SaveFilesError } from '../src/index.ts';
import { schema } from './schema.ts';
import fixture from './fixtures/save-names.json' with { type: 'json' };

describe('save files', () => {
  const files = (...names: string[]) => names.map((name) => ({ name, bytes: new Uint8Array() }));

  it('accepts and refuses the names of the shared fixture', () => {
    for (const ok of fixture.accept) expect(() => checkSaveFiles(files(ok)), ok).not.toThrow();
    for (const bad of fixture.refuse) expect(() => checkSaveFiles(files(bad)), bad).toThrow(SaveFilesError);
  });

  it('refuses names that differ only in case, in the fixture lists', () => {
    for (const ok of fixture.acceptLists) expect(() => checkSaveFiles(files(...ok)), ok.join()).not.toThrow();
    for (const bad of fixture.refuseLists) expect(() => checkSaveFiles(files(...bad)), bad.join()).toThrow(/twice/);
  });

  it('limits the count and the total size of a list', () => {
    const { maxFiles, maxTotalBytes } = fixture.limits;
    const many = (n: number) => files(...Array.from({ length: n }, (_, i) => `f${i}`));
    expect(() => checkSaveFiles(many(maxFiles))).not.toThrow();
    expect(() => checkSaveFiles(many(maxFiles + 1))).toThrow(/at most/);
    const big = (n: number) => [{ name: 'big', bytes: new Uint8Array(n) }];
    expect(() => checkSaveFiles(big(maxTotalBytes))).not.toThrow();
    expect(() => checkSaveFiles(big(maxTotalBytes + 1))).toThrow(/at most/);
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

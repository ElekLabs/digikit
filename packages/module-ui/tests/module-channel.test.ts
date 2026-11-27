import { describe, expect, it } from 'vitest';
import { createMemoryModuleChannel, type ModuleChannelEvent } from '../src/index.ts';
import { schema } from './schema.ts';

// Whatever crosses a sandbox boundary is copied with structured clone: every result and event must survive it unchanged.
describe('ModuleChannel results are structured-clone data', () => {
  it('copies every result and every event unchanged', async () => {
    const { channel } = createMemoryModuleChannel(schema, { initial: { gain: 0.4, name: 'ab' }, name: 'Typed' });
    const events: ModuleChannelEvent[] = [];
    channel.subscribe((e) => events.push(e));
    const results: unknown[] = [
      await channel.describeSettings(),
      await channel.getSetting('gain'),
      await channel.getSetting('flag'),
      await channel.putSetting('name', Uint8Array.of(0x41), 1),
      await channel.putSetting('name', Uint8Array.of(0x41), 0),
      await channel.getSetting('nope'),
      await channel.resetSetting('name', 2),
      await channel.sendFile(7, 'a.dkbs', Uint8Array.of(1, 2, 3)),
      await channel.listFiles(),
      await channel.readFile('a.dkbs'),
      await channel.removeFile('a.dkbs'),
    ];
    expect(events).toEqual([{ kind: 'file-progress', job: 7, done: 3, total: 3 }]);
    for (const value of [...results, ...events]) expect(structuredClone(value)).toEqual(value);
    expect(results.some((r) => r instanceof Uint8Array)).toBe(true);
  });
});

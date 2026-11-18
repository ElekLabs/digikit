import { describe, expect, test } from 'vitest';
import { unwrapBackup, wrapBackup } from '../src/backup-file.ts';
import { RpcData } from '../src/rpcdata.ts';
import type { RpcPort } from '../src/rpcfiles.ts';
import { FakeDataApi } from './data-api-fake.ts';
import { image } from './slot-image.ts';

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex');
const be = (n: number) => Buffer.from([n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
const stub = (reply: Uint8Array): RpcPort => ({ rpc: async () => reply });
const ignore = () => {};

describe('the Data API requests', () => {
  test('list asks for /samples from index 0 to the end, not only existing', async () => {
    const fake = new FakeDataApi();
    await new RpcData(fake).list();
    expect(fake.sent[0]!.type).toBe(0x53);
    expect(hex(fake.sent[0]!.payload)).toBe('2f73616d706c657300' + '00000000' + '00000000' + '00');
  });
  test('read open, partial and close', async () => {
    const fake = new FakeDataApi();
    fake.load(3, image('a', [1, 2]));
    const d = new RpcData(fake);
    await d.readOpen(3);
    expect(hex(fake.sent[0]!.payload)).toBe('2f73616d706c65732f3300' + '00004000' + '00');
    await d.readPartial(7, 2).catch(ignore);
    expect(hex(fake.sent[1]!.payload)).toBe('0000000700000002');
    await d.readClose(7).catch(ignore);
    expect(hex(fake.sent[2]!.payload)).toBe('00000007');
  });
  test('write open puts the size before the path', async () => {
    const fake = new FakeDataApi();
    await new RpcData(fake).writeOpen(3, 0x4b);
    expect(hex(fake.sent[0]!.payload)).toBe('0000004b' + '2f73616d706c65732f3300');
  });
  test('write partial carries job, seq, chunk CRC, length and the data', async () => {
    const fake = new FakeDataApi();
    await new RpcData(fake).writePartial(7, 0, Uint8Array.of(1, 2, 3)).catch(ignore);
    expect(hex(fake.sent[0]!.payload)).toBe('00000007' + '00000000' + '5502a6f0' + '00000003' + '010203');
  });
  test('write close and clear', async () => {
    const fake = new FakeDataApi();
    const d = new RpcData(fake);
    await d.writeClose(7).catch(ignore);
    expect(hex(fake.sent[0]!.payload)).toBe('00000007' + '00000000');
    await d.clear(5);
    expect(hex(fake.sent[1]!.payload)).toBe('2f73616d706c65732f3500');
  });
});

describe('the Data API replies', () => {
  const entry = (name: string, slot: number, size: number, exists: number) =>
    Buffer.concat([
      Buffer.from(`${name}\0`),
      Buffer.from([0, 2]),
      be(slot),
      be(size),
      Buffer.from([0, 0x7e, exists, 0]),
    ]);
  const list = (count: number, ...entries: Buffer[]) =>
    Buffer.concat([Buffer.from([1]), be(0), be(65), be(count), ...entries]);

  test('a list reply gives one row per slot, empty slots unused', async () => {
    const d = new RpcData(stub(list(2, entry('kick', 1, 0x44, 1), entry('', 2, 0, 0))));
    expect(await d.list()).toEqual([
      { slot: 1, name: 'kick', size: 0x44, used: true },
      { slot: 2, name: '', size: 0, used: false },
    ]);
  });
  test('a list reply with slot 65 or 65 entries is a protocol error', async () => {
    await expect(new RpcData(stub(list(1, entry('x', 65, 0, 0)))).list()).rejects.toMatchObject({ kind: 'protocol' });
    await expect(new RpcData(stub(list(65))).list()).rejects.toMatchObject({ kind: 'protocol' });
  });
  test('an error reply is a device error with its text, an empty reply a protocol error', async () => {
    const bad = Buffer.concat([Buffer.from([0]), Buffer.from('Invalid job id\0')]);
    await expect(new RpcData(stub(bad)).readClose(1)).rejects.toMatchObject({
      kind: 'device',
      message: expect.stringContaining('Invalid job id'),
    });
    await expect(new RpcData(stub(new Uint8Array(0))).readClose(1)).rejects.toMatchObject({ kind: 'protocol' });
  });
  test('a read partial with a wrong chunk CRC or for another chunk is refused', async () => {
    const reply = (seq: number, crc: number) =>
      Buffer.concat([
        Buffer.from([1]),
        be(7),
        be(seq),
        be(0),
        Buffer.from([1]),
        be(crc),
        be(3),
        Buffer.from([1, 2, 3]),
      ]);
    await expect(new RpcData(stub(reply(2, 0x5502a6f0))).readPartial(7, 2)).resolves.toMatchObject({ complete: true });
    await expect(new RpcData(stub(reply(2, 1))).readPartial(7, 2)).rejects.toThrow(/CRC/);
    await expect(new RpcData(stub(reply(3, 0x5502a6f0))).readPartial(7, 2)).rejects.toThrow(/another job or chunk/);
  });
});

describe('a whole slot over the fake firmware', () => {
  test('read file returns a backup file that unwraps to the loaded image, with ReadPartial seqs 1..n, and closes the job', async () => {
    const fake = new FakeDataApi();
    const img = image(
      'kick',
      Array.from({ length: 12000 }, (_, i) => i & 0x7fff),
    );
    fake.load(3, img);
    const file = await new RpcData(fake).readFile(3);
    expect(unwrapBackup(file)).toEqual({ slot: 3, image: img });
    const seqs = fake.sent.filter((r) => r.type === 0x55).map((r) => Buffer.from(r.payload).readUInt32BE(4));
    expect(seqs).toEqual(Array.from({ length: seqs.length }, (_, i) => i + 1));
    expect(seqs.length).toBeGreaterThan(1);
    expect(fake.open).toBeUndefined();
    await expect(new RpcData(fake).readClose(1)).rejects.toThrow(/Invalid job id/);
  });
  test('a slot number outside 1..64 sends nothing', async () => {
    const fake = new FakeDataApi();
    const d = new RpcData(fake);
    await expect(d.clear(0)).rejects.toMatchObject({ kind: 'usage' });
    await expect(d.clear(65)).rejects.toMatchObject({ kind: 'usage' });
    await expect(d.readOpen(1.5)).rejects.toMatchObject({ kind: 'usage' });
    await expect(d.writeOpen(65, 1)).rejects.toMatchObject({ kind: 'usage' });
    expect(fake.sent.length).toBe(0);
  });
  test('write file refuses before WriteOpen: another slot, a bad footer CRC, bytes after the footer', async () => {
    const fake = new FakeDataApi();
    const d = new RpcData(fake);
    const img = image('a', [1, 2, 3]);
    await expect(d.writeFile(3, wrapBackup(2, img))).rejects.toThrow(/slot 2, not slot 3/);
    const f = wrapBackup(3, img);
    const crc = Uint8Array.from(f, (x, i) => (i === f.length - 12 ? x ^ 1 : x));
    await expect(d.writeFile(3, crc)).rejects.toThrow(/CRC/);
    const tail = Uint8Array.from([...f, 1, 2, 3, 4, 5]);
    await expect(d.writeFile(3, tail)).rejects.toThrow(/bytes after its footer/);
    expect(fake.sent.length).toBe(0);
  });
  test('write file sends chunks of at least the header, totals the file size, then closes; other slots stay zero', async () => {
    const fake = new FakeDataApi();
    const img = image(
      'snare',
      Array.from({ length: 100 }, (_, i) => i + 1),
    );
    const file = wrapBackup(3, img);
    await new RpcData(fake, { chunk: 40 }).writeFile(3, file);
    const parts = fake.sent.filter((r) => r.type === 0x58);
    expect(parts.map((r) => Buffer.from(r.payload).readUInt32BE(4))).toEqual(
      Array.from({ length: parts.length }, (_, i) => i),
    );
    const lens = parts.map((r) => Buffer.from(r.payload).readUInt32BE(12));
    expect(lens[0]).toBeGreaterThanOrEqual(31);
    expect(lens.reduce((a, b) => a + b, 0)).toBe(31 + img.length + 12);
    expect(fake.sent.at(-1)!.type).toBe(0x59);
    expect(hex(fake.slotBytes(3).subarray(0, img.length))).toBe(hex(img));
    for (const s of [1, 2, 4, 64, 65, 70]) expect(fake.slotBytes(s).every((b) => b === 0)).toBe(true);
  });
  test('a body end off the chunk boundary: the footer is its own last chunk and the slot lists as used', async () => {
    const fake = new FakeDataApi();
    const img = image(
      'long',
      Array.from({ length: 48000 }, (_, i) => i & 0x7fff),
    );
    expect(img.length).toBe(96064);
    await new RpcData(fake, { chunk: 2048 }).writeFile(3, wrapBackup(3, img));
    const lens = fake.sent.filter((r) => r.type === 0x58).map((r) => Buffer.from(r.payload).readUInt32BE(12));
    expect(lens.at(-1)).toBe(12);
    expect(lens.slice(0, -1).reduce((a, b) => a + b, 0)).toBe(31 + img.length);
    expect(fake.used(3)).toBe(true);
    expect((await new RpcData(fake).list()).find((r) => r.slot === 3)?.used).toBe(true);
  });
});

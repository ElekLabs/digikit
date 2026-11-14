// The stock MidiRpc file client (rpcfiles.ts) against a fake Digitakt II that answers the FsSample commands byte for byte (docs/MIDI-SYSEX-RPC.md
// section 12): framing, ReadDir entries, StorageSpace of 17 and 20 bytes, folders, a write in chunks with the byte count at close, one writer at a time (a fake assumption),
// the device's hash over the file plus a 16-byte trailer, and a verify that fails when the device stores something else.
import assert from 'node:assert/strict';
import { DkSession, decode8in7 } from '@digi/dk-session';
import { RPC_CHUNK, RPC_FILE, RpcFiles, pathBytes, putVerified, verifyWritten } from '@digi/dk-session/rpcfiles';
import { test } from 'vitest';
import { FakeDrive, hashOf } from './support/fakedrive.ts';

const setup = (opts?: ConstructorParameters<typeof FakeDrive>[0]) => {
  const dev = new FakeDrive(opts);
  const session = new DkSession(dev, { timeoutMs: 50 });
  return { dev, session, files: new RpcFiles(session, { chunk: 100, timeoutMs: 50 }) };
};
const data = (n: number) => Uint8Array.from({ length: n }, (_, i) => (i * 7 + 3) & 255);

test('paths are absolute C-strings under 256 bytes', () => {
  assert.deepEqual([...pathBytes('a/b')], [...Buffer.from('/a/b\0')]);
  assert.deepEqual([...pathBytes('/a')], [...Buffer.from('/a\0')]);
  assert.throws(() => pathBytes('x'.repeat(256)), /too long/);
  assert.doesNotThrow(() => pathBytes('x'.repeat(254)));
  assert.throws(() => pathBytes('a\0b'), /NUL/);
});

test('StorageSpace: 17 bytes, or 20 with the numbers last', async () => {
  for (const spaceLen of [17, 20]) {
    const { files } = setup({ spaceLen });
    assert.deepEqual(await files.space(), { free: 1000n, total: 5000n });
  }
});

test('mkdir -p, ReadDir entries, rename, remove, rmdir', async () => {
  const { files } = setup();
  await files.mkdir('drums/kicks');
  assert.deepEqual(
    (await files.readDir('/')).map((e) => [e.name, e.kind]),
    [
      ['factory', 'D'],
      ['drums', 'D'],
    ],
  );
  assert.deepEqual(
    (await files.readDir('/drums')).map((e) => e.name),
    ['kicks'],
  );
  await files.write('drums/k1', data(10));
  const [k1] = (await files.readDir('/drums')).filter((e) => e.kind === 'F');
  assert.equal(k1.size, 10);
  await files.rename('/drums/k1', '/drums/kicks/k1');
  assert.deepEqual(
    (await files.readDir('/drums/kicks')).map((e) => e.name),
    ['k1'],
  );
  await assert.rejects(files.rmdir('drums/kicks'), /status 0/);
  await files.remove('drums/kicks/k1');
  await files.rmdir('drums/kicks');
  assert.deepEqual(await files.readDir('/drums'), []);
});

test('write: chunks at sequential offsets, the byte count at close, the size without the trailer', async () => {
  const { files, dev } = setup();
  const d = data(250);
  const seen: number[][] = [];
  const closed = await files.write('s/take', d, (sent, total) => seen.push([sent, total]));
  assert.deepEqual(seen, [
    [100, 250],
    [200, 250],
    [250, 250],
  ]);
  assert.deepEqual(closed, { id: 6, size: 250 });
  assert.equal(dev.seen.filter((t) => t === RPC_FILE.WRITE_V1).length, 3);
  assert.equal(dev.files.get('/s/take')?.length, 266);
  assert.deepEqual(await files.info('/s/take'), {
    found: true,
    size: 250,
    hash: hashOf(dev.files.get('/s/take') as Uint8Array),
  });
  assert.deepEqual(await files.info('/s/none'), { found: false, size: 0, hash: 0 });
  // an empty file
  assert.equal((await files.write('s/empty', new Uint8Array(0))).size, 0);
});

test("the chunk of a write: the default, the link's own (rpcChunk of the transport), or the one asked for", async () => {
  const writes = async (rpcChunk: number | undefined, options?: { chunk?: number }) => {
    const dev = new FakeDrive();
    if (rpcChunk !== undefined) dev.rpcChunk = rpcChunk;
    const session = new DkSession(dev, { timeoutMs: 50 });
    assert.equal(session.rpcChunk, rpcChunk);
    await new RpcFiles(session, { timeoutMs: 50, ...options }).write('s/f', data(3 * RPC_CHUNK + 1));
    return dev.seen.filter((t) => t === RPC_FILE.WRITE_V1).length;
  };
  assert.equal(await writes(undefined), 4); // a device link: RPC_CHUNK
  assert.equal(await writes(4 * RPC_CHUNK), 1); // the emulator's USB
  assert.equal(await writes(4 * RPC_CHUNK, { chunk: RPC_CHUNK }), 4); // the caller's own choice wins
});

test('a root-level file is sent with its leading slash (a bare name would fail on the device)', async () => {
  const { files, dev } = setup();
  await files.write('kick', data(8));
  assert.ok(dev.files.has('/kick'));
  assert.ok(!dev.dirs.has('/kick'));
});

test('a failed write closes its writer (the fake allows one at a time) so the next open works', async () => {
  const { files, dev } = setup();
  const real = dev.send.bind(dev);
  let writes = 0;
  dev.send = (msg: Uint8Array) => {
    const body = decode8in7(msg.subarray(6, msg.length - 1));
    if (body[4] === RPC_FILE.WRITE_V1 && ++writes === 2) return; // the second chunk is never answered
    real(msg);
  };
  await assert.rejects(files.write('s/lost', data(250)), /no reply/);
  assert.equal(dev.writer, null);
  dev.send = real;
  await files.write('s/next', data(5));
  assert.ok(dev.files.has('/s/next'));
});

test('verify: the device hash covers the stored file and the trailer; a different content fails', async () => {
  const { files } = setup();
  const d = data(300);
  const { closed, verified } = await putVerified(files, 'v/a', d, hashOf);
  assert.equal(closed.size, 300);
  assert.equal(verified.ok, true);
  assert.equal(verified.firmware, verified.host);
  assert.match(verified.compared, /zero trailer/);
  const other = Uint8Array.from(d);
  other[17] ^= 1;
  const bad = await verifyWritten(files, '/v/a', other, hashOf);
  assert.equal(bad.ok, false);
  assert.notEqual(bad.firmware, bad.host);
  const none = await verifyWritten(files, '/v/zzz', d, hashOf);
  assert.equal(none.ok, false);
  assert.equal(none.firmware, null);
});

test('verify: a device that stores other bytes than it was sent fails', async () => {
  const { files } = setup({ corrupt: true });
  const { verified } = await putVerified(files, 'v/b', data(64), hashOf);
  assert.equal(verified.ok, false);
  assert.equal(verified.size, 64);
});

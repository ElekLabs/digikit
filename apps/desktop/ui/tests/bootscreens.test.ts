import assert from 'node:assert/strict';
import { test } from 'vitest';
import { preflightBootscreen, readBootscreen, removeBootscreen, uploadBootscreen } from '../src/bootscreens-core.ts';

function asset() {
  const b = new Uint8Array(40);
  const h = new DataView(b.buffer);
  b.set([68, 75, 66, 83]);
  h.setUint16(4, 1);
  h.setUint16(6, 1);
  h.setUint32(8, 30);
  return b;
}

test('header preflight accepts a bounded v1 header and rejects contradictions without parsing records', () => {
  assert.deepEqual(preflightBootscreen(asset()), { records: 1, ticks: 30 });
  for (const [offset, value] of [
    [0, 0],
    [5, 2],
    [7, 0],
    [8, 255],
    [11, 0],
  ]) {
    const b = asset();
    b[offset] = value;
    assert.throws(() => preflightBootscreen(b));
  }
  const b = asset();
  new DataView(b.buffer).setUint16(6, 31);
  assert.throws(() => preflightBootscreen(b));
  assert.throws(() => preflightBootscreen(asset().subarray(0, 23)));
});

test('file size is bounded before reading and invalid selection cannot produce a candidate', async () => {
  let reads = 0;
  const file = {
    name: 'intro.dkbs',
    size: 65537,
    async arrayBuffer() {
      reads++;
      return asset().buffer;
    },
  };
  await assert.rejects(readBootscreen(file));
  file.size = 15;
  await assert.rejects(readBootscreen(file));
  assert.equal(reads, 0);
  file.size = 40;
  assert.equal((await readBootscreen(file)).ticks, 30);
  assert.equal(reads, 1);
});

test('upload replaces the fixed path then freshly verifies the same bytes, without selection', async () => {
  const bytes = asset();
  const calls: string[] = [];
  await uploadBootscreen(
    {
      async put(path, data, progress) {
        calls.push(`put:${path}`);
        assert.equal(data, bytes);
        progress?.(20, 40);
      },
      async verifyFile(path, data) {
        calls.push(`verify:${path}`);
        assert.equal(data, bytes);
        return true;
      },
    },
    bytes,
    (sent) => calls.push(`progress:${sent}`),
    () => calls.push('verifying'),
  );
  assert.deepEqual(calls, [
    'put:bootscreens/active.dkbs',
    'progress:20',
    'verifying',
    'verify:bootscreens/active.dkbs',
  ]);
});

test('bad headers send nothing and mismatched verification fails the upload', async () => {
  let puts = 0;
  const session = {
    async put() {
      puts++;
    },
    async verifyFile() {
      return false;
    },
  };
  await assert.rejects(
    uploadBootscreen(
      session,
      new Uint8Array(17),
      () => {},
      () => {},
    ),
  );
  assert.equal(puts, 0);
  await assert.rejects(
    uploadBootscreen(
      session,
      asset(),
      () => {},
      () => {},
    ),
    /Verification failed/,
  );
  assert.equal(puts, 1);
});

test('removal only deletes the active bootscreen', async () => {
  const paths: string[] = [];
  await removeBootscreen({
    async delete(path) {
      paths.push(path);
    },
  });
  assert.deepEqual(paths, ['bootscreens/active.dkbs']);
});

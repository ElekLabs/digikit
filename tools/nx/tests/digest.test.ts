import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { sha256File, sha256FileOrMissing, sha256Of } from '../digest.mjs';

const reference = (...parts: (string | Buffer)[]) =>
  createHash('sha256')
    .update(Buffer.concat(parts.map((p) => Buffer.from(p))))
    .digest('hex');

it('hashes parts as their concatenation, strings as UTF-8', () => {
  expect(sha256Of('ab', Buffer.from([0xc3, 0xa9]), 'é')).toBe(reference('ab', Buffer.from([0xc3, 0xa9]), 'é'));
  expect(sha256Of('a', 'b')).toBe(sha256Of('ab'));
});

it('hashes a file by its bytes, and a missing file as `missing` only in the stream form', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'digest-'));
  try {
    writeFileSync(join(dir, 'f'), Buffer.from([1, 2, 3]));
    expect(sha256File(join(dir, 'f'))).toBe(reference(Buffer.from([1, 2, 3])));
    expect(await sha256FileOrMissing(join(dir, 'f'))).toBe(sha256File(join(dir, 'f')));
    expect(await sha256FileOrMissing(join(dir, 'absent'))).toBe('missing');
    expect(() => sha256File(join(dir, 'absent'))).toThrow();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

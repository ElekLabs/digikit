// The one place that spells a sha256 of bytes or of a file, for the cache keys and byte manifests of the Nx tools.
import { createHash } from 'node:crypto';
import { createReadStream, readFileSync } from 'node:fs';

/** The sha256 hex of the parts in order (strings are UTF-8): the same as hashing their concatenation. */
export const sha256Of = (...parts) => {
  const hash = createHash('sha256');
  for (const part of parts) hash.update(part);
  return hash.digest('hex');
};

/** The sha256 hex of the bytes of a file. Throws when it cannot be read. */
export const sha256File = (path) => sha256Of(readFileSync(path));

/** The sha256 hex of a file read as a stream; `missing` when it cannot be read (a key line for an absent input). */
export const sha256FileOrMissing = (path) =>
  new Promise((done) => {
    const hash = createHash('sha256');
    const stream = createReadStream(path);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('end', () => done(hash.digest('hex')));
    stream.on('error', () => done('missing'));
  });

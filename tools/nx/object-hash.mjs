// The hash of a committed core object that follows its bytes: the content without `source_sha256` and `toolchain.cflags`, the
// fields that follow the source text and the flag list (a comment edit changes the first and no other field; a flag that changes
// code changes the sections). Used by device-bytes.mjs.
import { readFileSync } from 'node:fs';
import { sha256Of } from './digest.mjs';

export const objectHash = (path) => {
  const object = JSON.parse(readFileSync(path, 'utf8'));
  delete object.source_sha256;
  delete object.toolchain?.cflags;
  return sha256Of(JSON.stringify(object));
};

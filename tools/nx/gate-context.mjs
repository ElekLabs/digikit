// What the ownership gate reads from the checkout besides the Nx graph (tools/nx/device-owners.mjs):
// the `users` of a firmware declaration file and the device a device file names.
import { readFileSync } from 'node:fs';
import { join, matchesGlob } from 'node:path';
import { parse } from 'smol-toml';
import { deviceShorts, filesOf } from './device-files.mjs';

const readIfThere = (path) => {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return undefined;
  }
};

/** `{ portUsers(file), deviceOf(file) }` for the checkout at `root`. `portUsers` lists the project names a
 * `ports/fw/<point>.toml` file names in its `users` (empty when the file is gone); `deviceOf` gives the short name of
 * the device a device file belongs to, or undefined. */
export const gateContext = (root) => {
  const globs = deviceShorts(root).map((short) => ({ short, globs: filesOf(short, root) }));
  return {
    portUsers: (file) => {
      const text = readIfThere(join(root, file));
      if (text === undefined) return [];
      const entries = Object.values(parse(text)).flatMap((v) => (Array.isArray(v) ? v : []));
      return [...new Set(entries.flatMap((e) => (Array.isArray(e?.users) ? e.users : [])))];
    },
    deviceOf: (file) => globs.find(({ globs: g }) => g.some((glob) => matchesGlob(file, glob)))?.short,
  };
};

// What the ownership gate reads from the checkout besides the Nx graph (tools/nx/device-owners.mjs):
// the `users` of a firmware declaration file and the device a device file names.
import { spawnSync } from 'node:child_process';
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

const gitShow = (root, ref, file) => {
  const done = spawnSync('git', ['show', `${ref}:${file}`], { cwd: root, encoding: 'utf8' });
  return done.status === 0 ? done.stdout : undefined;
};

const entriesOf = (text) =>
  text === undefined ? [] : Object.values(parse(text)).flatMap((v) => (Array.isArray(v) ? v : []));

/** `{ portUsers(file), deviceOf(file) }` for the checkout at `root`. `portUsers` lists the project names a
 * `ports/fw/<point>.toml` file names in its `users` (empty when the file is gone); with `base` only the entries that differ
 * from the file at `base` count (a new, changed or removed declaration: the users of the others did not change); `deviceOf` gives the short name of
 * the device a device file belongs to, or undefined. */
export const gateContext = (root, base) => {
  const globs = deviceShorts(root).map((short) => ({ short, globs: filesOf(short, root) }));
  return {
    portUsers: (file) => {
      const text = readIfThere(join(root, file));
      if (text === undefined) return [];
      let entries = entriesOf(text);
      if (base) {
        const before = entriesOf(gitShow(root, base, file));
        const key = (e) => JSON.stringify(e, Object.keys(e ?? {}).sort());
        const was = new Set(before.map(key));
        const is = new Set(entries.map(key));
        entries = [...entries.filter((e) => !was.has(key(e))), ...before.filter((e) => !is.has(key(e)))];
      }
      return [...new Set(entries.flatMap((e) => (Array.isArray(e?.users) ? e.users : [])))];
    },
    deviceOf: (file) => globs.find(({ globs: g }) => g.some((glob) => matchesGlob(file, glob)))?.short,
  };
};

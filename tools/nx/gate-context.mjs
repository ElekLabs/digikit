// What the ownership gate reads from the checkout besides the Nx graph (tools/nx/device-owners.mjs):
// the `users` of a firmware declaration file and the device a device file names.
import { spawnSync } from 'node:child_process';
import { globSync, readFileSync } from 'node:fs';
import { join, matchesGlob } from 'node:path';
import { parse } from 'smol-toml';
import { deviceShorts, filesOf } from './device-files.mjs';
import { canonical, declaredUsers, portEntryUsers } from './port-entries.mjs';

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

const isGeneratedPort = (file, globs) =>
  file.startsWith('ports/') && globs.some(({ globs: g }) => g.some((glob) => matchesGlob(file, glob)));

/** `io` replaces the checkout reads in a test: `read(file)` (the file now, undefined when gone), `show(file)` (the file at `base`, undefined when absent),
 * `declarationTexts()`, `deviceGlobs()`.
 * `{ portUsers(file), portEntries(file), deviceOf(file) }` for the checkout at `root`. `portUsers` lists the project names a
 * `ports/fw/<point>.toml` file names in its `users` (empty when the file is gone); with `base` only the entries that differ
 * from the file at `base` count (a new, changed or removed declaration: the users of the others did not change); `deviceOf` gives the short name of
 * the device a device file belongs to, or undefined. */
export const gateContext = (root, base, io = {}) => {
  const {
    read = (file) => readIfThere(join(root, file)),
    show = (file) => gitShow(root, base, file),
    declarationTexts = () =>
      globSync('ports/fw/**/*.toml', { cwd: root }).map((f) => readFileSync(join(root, f), 'utf8')),
    deviceGlobs = () => deviceShorts(root).map((short) => ({ short, globs: filesOf(short, root) })),
  } = io;
  const globs = deviceGlobs();
  return {
    portUsers: (file) => {
      const text = read(file);
      if (text === undefined) return [];
      let entries = entriesOf(text);
      if (base) {
        const before = entriesOf(show(file));
        const was = new Set(before.map(canonical));
        const is = new Set(entries.map(canonical));
        entries = [...entries.filter((e) => !was.has(canonical(e))), ...before.filter((e) => !is.has(canonical(e)))];
      }
      return [...new Set(entries.flatMap((e) => (Array.isArray(e?.users) ? e.users : [])))];
    },
    // A generated port file (`ports/<dev>-<version>.toml`): { users, wide } of the entries that differ from `base`
    // (port-entries.mjs); undefined for any other file.
    portEntries: (file) => {
      if (!isGeneratedPort(file, globs)) return undefined;
      const head = read(file);
      if (head === undefined || base === undefined) return { users: [], wide: true };
      const declared = declaredUsers(declarationTexts());
      return portEntryUsers({ head, base: show(file), declared });
    },
    deviceOf: (file) => globs.find(({ globs: g }) => g.some((glob) => matchesGlob(file, glob)))?.short,
  };
};

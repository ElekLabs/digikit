// The one directory walker of the Nx tools.
import { readdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The regular files under `dir` (absolute when `dir` is), in directory order. Symbolic links are not followed.
 * A directory that cannot be read (missing, no permission) is skipped; with `strict` its error is thrown instead.
 */
export const walkFiles = (dir, { strict = false } = {}) => {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch (error) {
    if (strict) throw error;
    return [];
  }
  return entries.flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return walkFiles(path, { strict });
    return e.isFile() ? [path] : [];
  });
};

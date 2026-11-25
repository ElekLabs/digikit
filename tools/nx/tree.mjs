// The one directory walker of the Nx tools.
import { readdirSync } from 'node:fs';
import { join } from 'node:path';

/** The regular files under `dir` (absolute when `dir` is), in directory order; none when `dir` is missing or unreadable. Symbolic links are not followed. */
export const walkFiles = (dir) => {
  let entries;
  try {
    entries = readdirSync(dir, { recursive: true, withFileTypes: true });
  } catch {
    return [];
  }
  return entries.filter((e) => e.isFile()).map((e) => join(e.parentPath, e.name));
};

// The tracked DKO inventory used by the core checksum producer and the E8 byte manifest.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { read } from './module-manifests.mjs';

export const trackedObjects = (root) => {
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--', ':(glob)**/build/*.dko.json'], {
    cwd: root,
    encoding: 'utf8',
  })
    .split('\0')
    .filter(Boolean);
  const manifests = new Map();
  const objects = new Map();
  for (const path of files) {
    if (objects.has(path)) throw new Error(`Duplicate tracked object ${path}`);
    let dir = dirname(dirname(path));
    while (!manifests.has(dir) && !existsSync(join(root, dir, 'module.toml'))) {
      if (dir === '.') throw new Error(`No module manifest for ${path}`);
      dir = dirname(dir);
    }
    if (!manifests.has(dir)) manifests.set(dir, read(root, join(dir, 'module.toml')).module);
    objects.set(path, { dir, category: manifests.get(dir).category });
  }
  return objects;
};

// The manifest invokes this worker under benchlock. Each module gets a separate --out directory
// because independently built modules can use the same relative object filenames.
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const [out, ...dirs] = process.argv.slice(2);
  for (const dir of dirs)
    execFileSync(
      join(root, 'dist/patchkit/patchkit'),
      ['--root', root, 'build', '--force', '--out', join(out, dir), dir],
      {
        cwd: root,
        stdio: ['ignore', 'ignore', 'inherit'],
      },
    );
}

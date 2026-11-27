// Check (a command of `patchkit:repo-check`): the build/gen/gallery.json of each module lists every device of its manifest.
// A module whose gallery lacks a device until `module:check` regenerates it in the next device window lists its gaps in `gallery-pending.json`
// in its own directory (a JSON array of the messages): the gaps found must be exactly those. Delete the file when the gallery is fixed.
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { galleryGaps, read } from './module-manifests.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const found = {};
const pending = {};
const walk = (dir) => {
  for (const e of readdirSync(join(repo, dir), { withFileTypes: true })) {
    if (!e.isDirectory() || ['build', 'node_modules', 'src', 'tests'].includes(e.name)) continue;
    const sub = join(dir, e.name);
    if (existsSync(join(repo, sub, 'module.toml'))) {
      const manifest = read(repo, join(sub, 'module.toml'));
      const gaps = galleryGaps(join(repo, sub), manifest).map((g) => g.replace(repo, ''));
      if (gaps.length) found[manifest.module.id] = gaps;
      const marker = join(repo, sub, 'gallery-pending.json');
      if (existsSync(marker)) pending[manifest.module.id] = JSON.parse(readFileSync(marker, 'utf8'));
    }
    walk(sub);
  }
};
for (const root of ['modules', 'runtime', 'services']) walk(root);
if (JSON.stringify(found) !== JSON.stringify(pending)) {
  console.error('check-gallery: the galleries differ from the gallery-pending.json files');
  console.error(`  found:   ${JSON.stringify(found)}`);
  console.error(`  pending: ${JSON.stringify(pending)}`);
  process.exit(1);
}
console.log('check-gallery: every gallery lists the devices of its manifest');

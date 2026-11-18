// Writes apps/web/public/modules/bind.json (`{ports, fw}`): what the service binder reads beside the user's stock image. The site build
// (gen-modules.mjs) writes it with the module bundle; the Nx target `@digi/web:bind-bundle` runs this file alone for the desktop workbench.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildBindBundle } from '../../../packages/workbench-core/src/node/bind-bundle.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');

export function writeBindBundle(out = join(here, '..', 'public', 'modules')) {
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'bind.json'), JSON.stringify(buildBindBundle(repoRoot)));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  writeBindBundle();
  console.log('wrote apps/web/public/modules/bind.json');
}

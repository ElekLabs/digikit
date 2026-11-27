// Writes runtime/core/vectors/wire-vectors.json. Usage: node scripts/write-vectors.ts
import { writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cases } from '../src/vectors.ts';

const out = resolve(dirname(fileURLToPath(import.meta.url)), '../../../runtime/core/vectors/wire-vectors.json');
const all = cases();
writeFileSync(out, `${JSON.stringify({ schema: 'tests/fixtures/wire/project_manager.proto', cases: all }, null, 1)}\n`);
console.log(`wrote ${all.length} cases to ${out}`);

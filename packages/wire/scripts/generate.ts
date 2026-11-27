// Writes src/gen/*_pb.ts (protobuf-es) for the schemas of tests/fixtures/wire: `patchkit wire descriptor` makes the descriptor set
// (protox, no protoc), this script wraps it in a CodeGeneratorRequest and runs protoc-gen-es on it. No protoc or buf binary.
// Usage: node scripts/generate.ts [--patchkit PATH]
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { protocGenEs } from './protoc-es.ts';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../..');
const flag = process.argv.indexOf('--patchkit');
const patchkit = flag > 0 ? (process.argv[flag + 1] as string) : join(root, 'target/debug/patchkit');
const proto = 'project_manager.proto';

const dir = mkdtempSync(join(tmpdir(), 'wire-gen-'));
const setFile = join(dir, 'set.bin');
execFileSync(patchkit, [
  'wire',
  'descriptor',
  '--proto',
  proto,
  '--include',
  join(root, 'tests/fixtures/wire'),
  '--include',
  join(root, 'sdk/proto'),
  '--out',
  setFile,
]);
const files = protocGenEs(readFileSync(setFile), [proto, 'dk/options.proto']);
for (const f of files) {
  const path = join(here, '../src/gen', f.name);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, f.content);
}
console.log(`wrote ${files.map((f) => f.name).join(', ')}`);

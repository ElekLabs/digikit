// Writes src/gen/*_pb.ts (protobuf-es) for the schemas of tests/fixtures/wire: `patchkit wire descriptor` makes the descriptor set
// (protox, no protoc), this script wraps it in a CodeGeneratorRequest and runs protoc-gen-es on it. No protoc or buf binary.
// Usage: node scripts/generate.ts [--patchkit PATH]
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { create, fromBinary, toBinary } from '@bufbuild/protobuf';
import { FileDescriptorSetSchema } from '@bufbuild/protobuf/wkt';
import { CodeGeneratorRequestSchema, CodeGeneratorResponseSchema } from '@bufbuild/protobuf/wkt';

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
const set = fromBinary(FileDescriptorSetSchema, readFileSync(setFile));
const request = create(CodeGeneratorRequestSchema, {
  fileToGenerate: [proto, 'dk/options.proto'],
  parameter: 'target=js+dts,import_extension=js',
  protoFile: set.file,
});
const plugin = join(here, '../node_modules/.bin/protoc-gen-es');
const run = spawnSync(plugin, { input: toBinary(CodeGeneratorRequestSchema, request), maxBuffer: 1 << 26 });
if (run.status !== 0) throw new Error(`protoc-gen-es failed: ${run.stderr}`);
const response = fromBinary(CodeGeneratorResponseSchema, run.stdout);
if (response.error) throw new Error(response.error);
for (const f of response.file) {
  const path = join(here, '../src/gen', f.name);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, f.content);
}
console.log(`wrote ${response.file.map((f) => f.name).join(', ')}`);

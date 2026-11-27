// Runs protoc-gen-es on a descriptor set (no protoc or buf binary): wraps the set in a CodeGeneratorRequest and returns the files.
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { create, fromBinary, toBinary } from '@bufbuild/protobuf';
import {
  CodeGeneratorRequestSchema,
  CodeGeneratorResponseSchema,
  FileDescriptorSetSchema,
} from '@bufbuild/protobuf/wkt';

const here = dirname(fileURLToPath(import.meta.url));

export function protocGenEs(setBytes: Uint8Array, protos: string[]): { name: string; content: string }[] {
  const set = fromBinary(FileDescriptorSetSchema, setBytes);
  const request = create(CodeGeneratorRequestSchema, {
    fileToGenerate: protos,
    parameter: 'target=js+dts,import_extension=js',
    protoFile: set.file,
  });
  const plugin = join(here, '../node_modules/.bin/protoc-gen-es');
  const run = spawnSync(plugin, { input: toBinary(CodeGeneratorRequestSchema, request), maxBuffer: 1 << 26 });
  if (run.status !== 0) throw new Error(`protoc-gen-es failed: ${run.stderr}`);
  const response = fromBinary(CodeGeneratorResponseSchema, run.stdout);
  if (response.error) throw new Error(response.error);
  return response.file.map((f) => ({ name: f.name, content: f.content }));
}

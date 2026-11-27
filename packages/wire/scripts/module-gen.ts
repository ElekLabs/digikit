// The protobuf-es files of one module's query schema: `patchkit workbench-ts` writes the descriptor set (protox) and runs this.
// Usage: node scripts/module-gen.ts --descriptor SET --proto NAME.proto --out DIR
// Writes DIR/NAME_pb.js and DIR/NAME_pb.d.ts. The import of dk/options_pb is the one the package holds (`@digi/wire/dk/options_pb.js`).
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { protocGenEs } from './protoc-es.ts';

const arg = (name: string) => {
  const i = process.argv.indexOf(name);
  const v = i > 0 ? process.argv[i + 1] : undefined;
  if (!v) throw new Error(`usage: module-gen.ts --descriptor SET --proto NAME.proto --out DIR (missing ${name})`);
  return v;
};
const proto = arg('--proto');
const out = arg('--out');
mkdirSync(out, { recursive: true });
for (const f of protocGenEs(readFileSync(arg('--descriptor')), [proto])) {
  writeFileSync(join(out, f.name), f.content.replaceAll('"./dk/options_pb.js"', '"@digi/wire/dk/options_pb.js"'));
}

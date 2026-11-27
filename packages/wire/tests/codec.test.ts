import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fromBinary, toJson, type DescMessage } from '@bufbuild/protobuf';
import { beforeAll, describe, expect, it } from 'vitest';
import { CoverageSchema, ListReplySchema, ListRequestSchema, ProjectSchema } from '../src/gen/project_manager_pb.js';
import type { Case } from '../src/vectors.ts';

const root = resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const schemas: Record<string, DescMessage> = {
  ListRequest: ListRequestSchema,
  Project: ProjectSchema,
  ListReply: ListReplySchema,
  Coverage: CoverageSchema,
};
const bytes = (hex: string) => Uint8Array.from(Buffer.from(hex, 'hex'));

// The shared vectors are the Rust test's input as well (packages/patchkit/tests/wire_roundtrip.rs). Here protobuf-es reads what the
// generated C codec writes.
const cases: Case[] = JSON.parse(readFileSync(join(root, 'runtime/core/vectors/wire-vectors.json'), 'utf8')).cases;
let results: string[];

beforeAll(() => {
  const patchkit = [
    process.env.PATCHKIT,
    join(root, 'dist/patchkit/patchkit'),
    join(root, 'target/debug/patchkit'),
  ].find((p) => p && existsSync(p));
  if (!patchkit) throw new Error('patchkit is not built: pnpm nx:pilot run patchkit:cli');
  const dir = mkdtempSync(join(tmpdir(), 'wire-codec-'));
  const wire = join(root, 'tests/fixtures/wire');
  execFileSync(patchkit, [
    'wire',
    'c',
    '--proto',
    'project_manager.proto',
    '--prefix',
    'pm',
    '--include',
    wire,
    '--include',
    join(root, 'sdk/proto'),
    '--out',
    join(dir, 'pm.h'),
    '--source-out',
    join(dir, 'pm.c'),
  ]);
  const exe = join(dir, 'harness');
  execFileSync('cc', [
    '-std=c11',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-I',
    dir,
    '-I',
    join(root, 'sdk/include'),
    '-o',
    exe,
    join(wire, 'harness.c'),
    join(root, 'sdk/src/wire/wire.c'),
  ]);
  const input = cases.map((c) => `${c.message} ${c.input_hex}\n`).join('');
  results = execFileSync(exe, { input, maxBuffer: 1 << 26 })
    .toString()
    .split('\n')
    .slice(0, -1);
});

describe('query codec, C against protobuf-es', () => {
  it('has the vectors it needs', () => {
    expect(cases.length).toBeGreaterThan(200);
    expect(cases.filter((c) => c.expect === 'refuse').length).toBeGreaterThan(10);
  });

  it('writes bytes that protobuf-es reads as the original message', () => {
    for (const [i, c] of cases.entries()) {
      if (c.expect !== 'ok') continue;
      const schema = schemas[c.message] as DescMessage;
      expect(results[i], c.name).not.toMatch(/^[A-Z]/);
      expect(toJson(schema, fromBinary(schema, bytes(results[i] as string))), c.name).toEqual(c.json);
    }
  });

  it('reads each input as protobuf-es does, so the vectors mean what they say', () => {
    for (const c of cases) {
      if (c.expect !== 'ok') continue;
      const schema = schemas[c.message] as DescMessage;
      expect(toJson(schema, fromBinary(schema, bytes(c.input_hex))), c.name).toEqual(c.json);
    }
  });

  it('refuses what is over its bound, truncated or malformed', () => {
    for (const [i, c] of cases.entries()) {
      if (c.expect === 'refuse') expect(results[i], c.name).toBe('REFUSE');
    }
  });
});

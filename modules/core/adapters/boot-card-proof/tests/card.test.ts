import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, test } from 'vitest';

const module = path.resolve(import.meta.dirname, '..');
const root = path.resolve(module, '../../../..');
let directory: string, binary: string;
beforeAll(() => {
  directory = fs.mkdtempSync(path.join(os.tmpdir(), 'boot-card-host-'));
  binary = path.join(directory, 'card-host');
  execFileSync(
    'cc',
    [
      '-std=c11',
      '-Wall',
      '-Wextra',
      '-Werror',
      '-I',
      path.join(root, 'sdk/include'),
      '-I',
      path.join(root, 'modules/core/runtime/include'),
      path.join(module, 'src/card.c'),
      path.join(module, 'tests/card-host.c'),
      '-o',
      binary,
    ],
    { timeout: 10_000 },
  );
});
afterAll(() => fs.rmSync(directory, { recursive: true, force: true }));

test.each(['valid', 'actual', 'missing', 'short', 'oversized', 'allocation', 'negative', 'zero', 'overread'])(
  'owned provider lifetime and failure cleanup: %s',
  (mode) => {
    execFileSync(binary, [mode], { timeout: 2000 });
  },
);

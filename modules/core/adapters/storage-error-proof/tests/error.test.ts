import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, test } from 'vitest';

const module = path.resolve(import.meta.dirname, '..');
let directory: string, binary: string;
beforeAll(() => {
  directory = fs.mkdtempSync(path.join(os.tmpdir(), 'boot-io-host-'));
  binary = path.join(directory, 'error-host');
  execFileSync(
    'cc',
    [
      '-std=c11',
      '-Wall',
      '-Wextra',
      '-Werror',
      path.join(module, 'src/error.c'),
      path.join(module, 'tests/error-host.c'),
      '-o',
      binary,
    ],
    { timeout: 10_000 },
  );
});
afterAll(() => fs.rmSync(directory, { recursive: true, force: true }));

test.each([0, 1, 2, 3, 4, 5, 6, 7])('returned-error matching, range, and one-shot semantics: mode %i', (mode) => {
  execFileSync(binary, [String(mode)], { timeout: 2000 });
});

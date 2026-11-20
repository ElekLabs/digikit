import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'vitest';

test('the card guard: allow-list check, writer classes, boundaries, erase and counters (src/cf/guard.h, guard.c)', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'card-guard-'));
  try {
    const binary = path.join(directory, 'guard-host');
    const module = path.join(import.meta.dirname, '..');
    execFileSync(
      'cc',
      [
        '-std=c11', '-Wall', '-Wextra', '-Werror',
        '-I', path.join(module, 'src'),
        path.join(module, 'tests/guard-host.c'),
        path.join(module, 'src/cf/guard.c'),
        '-o', binary,
      ],
      { timeout: 25_000 },
    );
    execFileSync(binary, { timeout: 10_000 });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}, 40_000);

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'vitest';

test('the key subscribers: slots, refusals, the walk order and the first consumer (src/cf/key_subscribers.c)', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'key-subscribers-'));
  try {
    const binary = path.join(directory, 'key-subscribers-host');
    const module = path.join(import.meta.dirname, '..');
    execFileSync(
      'cc',
      [
        '-std=c11', '-Wall', '-Wextra', '-Werror',
        '-I', path.join(module, 'src'),
        path.join(module, 'tests/key-subscribers-host.c'),
        path.join(module, 'src/cf/key_subscribers.c'),
        '-o', binary,
      ],
      { timeout: 25_000 },
    );
    execFileSync(binary, { timeout: 10_000 });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}, 40_000);

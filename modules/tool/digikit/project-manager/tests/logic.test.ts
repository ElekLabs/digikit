import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'vitest';

test('project_text, built from the module C files', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'project-manager-'));
  try {
    const binary = path.join(directory, 'logic');
    const here = import.meta.dirname;
    execFileSync(
      'cc',
      [
        '-std=c11',
        '-Wall',
        '-Wextra',
        '-Werror',
        '-I',
        path.join(here, '../../../../../sdk/include'),
        path.join(here, 'logic-driver.c'),
        path.join(here, '../src/c/project_text.c'),
        '-o',
        binary,
      ],
      { timeout: 25_000 },
    );
    execFileSync(binary, { timeout: 10_000 });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}, 40_000);

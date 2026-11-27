import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { it } from 'vitest';

const root = fileURLToPath(new URL('../../../', import.meta.url));

function runHostC(source: string, ...extra: string[]) {
  const scratch = mkdtempSync(join(tmpdir(), 'digikit-sdk-host-'));
  try {
    const binary = join(scratch, 'host');
    execFileSync('cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', '-Isdk/include', source, ...extra, '-o', binary], {
      cwd: root,
      timeout: 25_000,
      stdio: 'pipe',
    });
    execFileSync(binary, { timeout: 10_000, stdio: 'pipe' });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

it('draws the page widgets inside their bounds', () => {
  runHostC('sdk/tests/page/widgets.c');
}, 40_000);

it('draws and clips the sample view', () => {
  runHostC('sdk/tests/sample/view.c');
}, 40_000);

it('reads and writes big-endian 32-bit fields', () => {
  runHostC('sdk/tests/core/bytes.c');
}, 40_000);

it('bounds every text helper to its buffer and formats numbers', () => {
  runHostC('sdk/tests/core/text.c', 'sdk/src/core/text.c');
}, 40_000);

it('joins, splits and names paths within the public limit', () => {
  runHostC('sdk/tests/core/path.c', 'sdk/src/core/path.c', 'sdk/src/core/text.c');
}, 40_000);

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'vitest';

const module = path.join(import.meta.dirname, '..');
const root = path.join(module, '../../../..');

// The stock section 7 and its SHA-256: the firmware file is not in the repository, so that part runs only when `out/sections` has it.
const section = path.join(root, 'out/sections/st-1.42/section_7_BLOB.bin');
const hash = /sha256 = "([0-9a-f]{64})"/.exec(fs.readFileSync(path.join(module, 'module.toml'), 'utf8'))?.[1];

test('the adapter C code: SHA-256, the section 6 pass-through, the section 7 unpack and every refusal', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'peer-host-'));
  try {
    const binary = path.join(directory, 'peer-host');
    execFileSync(
      'cc',
      [
        '-std=c11', '-Wall', '-Wextra', '-Werror',
        '-I', path.join(root, 'modules/core/fw/include'),
        '-I', path.join(module, 'src/cf'),
        '-I', path.join(module, 'build/gen'),
        path.join(module, 'tests/peer-host.c'),
        '-o', binary,
      ],
      { timeout: 25_000 },
    );
    const args = fs.existsSync(section) && hash ? [section, hash] : [];
    execFileSync(binary, args, { timeout: 20_000 });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}, 40_000);

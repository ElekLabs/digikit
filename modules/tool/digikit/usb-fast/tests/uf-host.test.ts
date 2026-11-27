import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'vitest';

// The real src/c/usb-fast.c on a fake SDK (uf-host.c): the stream parser, the nonce of the INFO reply, the speed-switch mask, the staging buffer and the session expiry.
test('usb-fast parser, nonce, staging buffer and session expiry, built from the module C file on a fake SDK', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'usb-fast-host-'));
  try {
    const binary = path.join(directory, 'uf-host');
    const here = import.meta.dirname;
    execFileSync(
      'cc',
      [
        '-std=c11',
        '-Wall',
        '-Wextra',
        '-Werror',
        // the module file asks for the target's `no-jump-tables` optimisation
        '-Wno-unknown-pragmas',
        '-I',
        path.join(here, '../../../../../sdk/include'),
        path.join(here, 'uf-host.c'),
        '-o',
        binary,
      ],
      { timeout: 25_000 },
    );
    execFileSync(binary, { timeout: 60_000, stdio: 'inherit' });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}, 120_000);

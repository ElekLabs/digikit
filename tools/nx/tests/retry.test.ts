import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { expect, it } from 'vitest';
import { retryDecision } from '../retry.mjs';

it('does not retry a real assertion failure, also beside a symptom', () => {
  expect(retryDecision({ status: 1, attempt: 1, output: 'AssertionError: expected 3 to be 4' })).toEqual({
    retry: false,
  });
  expect(
    retryDecision({
      status: 1,
      attempt: 1,
      output: 'AssertionError: expected 1 to equal 2\nemulator closed the connection',
    }),
  ).toEqual({ retry: false });
  expect(retryDecision({ status: 1, attempt: 1, output: 'TypeError: x is not a function' })).toEqual({ retry: false });
});

it('emu-slot keeps the exit status of a real failure with no rerun', () => {
  const slots = mkdtempSync(join(tmpdir(), 'digikit-slot-'));
  const done = spawnSync(
    'node',
    [
      'tools/devtools/emu-slot.mjs',
      '--',
      'node',
      '-e',
      "console.error('AssertionError: expected 1 to be 2');process.exit(3)",
    ],
    {
      encoding: 'utf8',
      env: { ...process.env, DIGI_EMU_SLOTS: '1', DIGI_EMU_SLOT_DIR: slots },
    },
  );
  rmSync(slots, { recursive: true, force: true });
  expect(done.status).toBe(3);
  expect(done.stderr).not.toContain('RETRY');
});

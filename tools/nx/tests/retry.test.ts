import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { expect, it } from 'vitest';
import { retrySummary } from '../gate-history.mjs';
import { classifyFailure, retryDecision } from '../retry.mjs';

const symptoms = [
  ['no new boot within 90 s (no reply from the device)', 'boot-timeout-wall'],
  ['Error: no new boot: not seen within 90000 ms of emulated time', 'boot-timeout-emulated'],
  ['Error: runtime lifecycle deadline exceeded', 'lifecycle-deadline'],
  ['Error: emulator closed the connection', 'connection-closed'],
  ['Error: remote release timed out', 'remote-release-timeout'],
] as const;

it('names each load symptom', () => {
  for (const [text, name] of symptoms) expect(classifyFailure(`FAIL x\n${text}\n`)).toBe(name);
});

it('sees a symptom through color codes', () => {
  expect(classifyFailure('\u001b[31mError: emulator closed the connection\u001b[39m')).toBe('connection-closed');
});

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

it('retries a symptom once, then fails', () => {
  const output = 'Error: runtime lifecycle deadline exceeded';
  expect(retryDecision({ status: 1, attempt: 1, output })).toEqual({ retry: true, symptom: 'lifecycle-deadline' });
  expect(retryDecision({ status: 1, attempt: 2, output }).retry).toBe(false);
  expect(retryDecision({ status: 0, attempt: 1, output }).retry).toBe(false);
});

it('emu-slot reruns a command once after a symptom, records it, and the summary lists it', () => {
  const dir = mkdtempSync(join(tmpdir(), 'digikit-retry-'));
  try {
    const marker = join(dir, 'ran');
    const script = `const fs=require('fs');if(fs.existsSync(${JSON.stringify(marker)}))process.exit(0);fs.writeFileSync(${JSON.stringify(marker)},'1');console.error('Error: emulator closed the connection');process.exit(1)`;
    const done = spawnSync('node', ['tools/devtools/emu-slot.mjs', '--', 'node', '-e', script], {
      encoding: 'utf8',
      env: {
        ...process.env,
        DIGI_EMU_SLOTS: '1',
        DIGI_TRACE_DIR: dir,
        NX_TASK_TARGET_PROJECT: 'p',
        NX_TASK_TARGET_TARGET: 't',
      },
    });
    expect(done.status).toBe(0);
    expect(done.stderr).toContain('RETRY p:t: load symptom "connection-closed"');
    const summary = retrySummary(dir);
    expect(summary).toContain('retried (1)');
    expect(summary).toContain('p:t  connection-closed  passed on retry');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('emu-slot keeps the exit status of a real failure with no rerun', () => {
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
      env: { ...process.env, DIGI_EMU_SLOTS: '1', DIGI_EMU_SLOT_DIR: join(tmpdir(), `digikit-slot-${process.pid}`) },
    },
  );
  expect(done.status).toBe(3);
  expect(done.stderr).not.toContain('RETRY');
});

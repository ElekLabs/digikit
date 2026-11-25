import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { buildRows, compareAudit, durationName, parseAudit, parseRunLog, summarize } from '../device-timings.mjs';

const log = readFileSync(new URL('./fixtures/nx-run-log.txt', import.meta.url), 'utf8');
const run = parseRunLog(log, '2026-11-25T19-41-12-527Z-81133.log');
const cmd = (file: string, dsp?: string) => ({
  options: {
    command: `node tools/devtools/emu-slot.mjs -- pnpm test:runtime ../../modules/${file}`,
    env: dsp ? { DIGI_EMU_DSP: dsp } : {},
  },
});
const graph = {
  nodes: {
    'core/dynfx': {
      data: {
        targets: {
          'test-device--alpha': cmd('x/one.device.test.ts', 'sharc'),
          'test-device--beta': cmd('x/one.device.test.ts', 'sharc'),
          pack: {},
        },
      },
    },
    'core/zero-tests': { data: { targets: { 'test-device--alpha': cmd('y/two.device.test.ts') } } },
  },
};

it('reads tasks, cache status and the run window from a launcher log', () => {
  expect(run.tasks.map((t) => [t.id, t.cached])).toContainEqual(['...e/zero-tests:test-device--alpha', true]);
  expect(run.failed).toEqual(['core/dynfx:test-device--beta']);
  expect(run.wall).toBe(100);
  expect(run.end! - run.start!).toBe(100000);
});

it('joins the recorded slot seconds to device halves, with mode and file, and flags files of another run', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dur-'));
  writeFileSync(join(dir, durationName('core/dynfx:test-device--alpha')), '90.5');
  writeFileSync(join(dir, durationName('core/zero-tests:test-device--alpha')), '10');
  const rows = buildRows(run, graph, dir);
  expect(rows.map((r) => r.id)).toEqual([
    'core/zero-tests:test-device--alpha',
    'core/dynfx:test-device--alpha',
    'core/dynfx:test-device--beta',
  ]);
  const alpha = rows[1];
  expect([alpha.mode, alpha.device, alpha.seconds, alpha.files]).toEqual([
    'coupled',
    'alpha',
    90.5,
    ['modules/x/one.device.test.ts'],
  ]);
  expect(alpha.outside).toBe(true); // the file was written now, the log window is 2026
  expect(rows[2].failed).toBe(true);
  const s = summarize(rows, run);
  expect(s.sum).toBe(90.5); // the cached half is not part of this run
});

it('lists files whose audit minutes are off by more than 2x', () => {
  const audit = parseAudit(
    '| file | project | halves | min before |\n|---|---|---|---|\n| one | dynfx | A | 9.0 | m |\n| other | dynfx | A | 1.0 | m |',
  );
  const rows = [
    { files: ['modules/x/one.device.test.ts'], seconds: 90, cached: false },
    { files: ['o/other.device.test.ts'], seconds: 60, cached: false },
  ];
  const out = compareAudit(rows as never, audit);
  expect(out).toContain('| one | 1.5 | 9.0 | 6.0x |');
  expect(out).not.toContain('other');
});

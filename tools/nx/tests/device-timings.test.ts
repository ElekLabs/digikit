import { mkdtempSync, readFileSync, utimesSync, writeFileSync } from 'node:fs';
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
  const put = (id: string, secs: string, at: number) => {
    const f = join(dir, durationName(id));
    writeFileSync(f, secs);
    utimesSync(f, at / 1000, at / 1000);
  };
  put('core/dynfx:test-device--alpha', '90.5', run.start! + 50000); // inside the window
  put('core/zero-tests:test-device--alpha', '10', run.end! + 3600000); // an hour later: another run
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
  expect(alpha.outside).toBe(false);
  expect(rows[0].outside).toBe(true);
  expect(rows[2].failed).toBe(true);
  const s = summarize(rows, run);
  expect(s.sum).toBe(90.5); // the cached half is not part of this run
});

it('leaves failed and outside rows out of the sums and the audit comparison', () => {
  const rows = [
    { files: ['x/one.device.test.ts'], seconds: 60, cached: false, mode: 'zero' },
    { files: ['x/one.device.test.ts'], seconds: 600, cached: false, failed: true, mode: 'zero' },
    { files: ['x/one.device.test.ts'], seconds: 600, cached: false, outside: true, mode: 'zero' },
  ];
  expect(summarize(rows as never, run).sum).toBe(60);
  const audit = parseAudit('| file | p | h | min |\n|---|---|---|---|\n| one | d | A | 5.0 | m |');
  expect(compareAudit(rows as never, audit)).toContain('| one | 1.0 | 5.0 | 5.0x |');
});

it('marks rows unverified when the run window is unknown', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dur-'));
  writeFileSync(join(dir, durationName('core/zero-tests:test-device--alpha')), '10');
  const unknown = parseRunLog(log, 'run.log'); // no timestamp in the name
  const rows = buildRows(unknown, graph, dir);
  expect(rows.filter((r) => r.seconds !== undefined).every((r) => r.unverified)).toBe(true);
  expect(summarize(rows, unknown).sum).toBe(0);
});

it('accepts test-device-all halves', () => {
  const g = {
    nodes: { 'core/zero-tests': { data: { targets: { 'test-device-all--alpha': cmd('y/two.device.test.ts') } } } },
  };
  const r = { ...run, tasks: [{ id: 'core/zero-tests:test-device-all--alpha', cached: false, failed: false }] };
  expect(buildRows(r, g, undefined).map((x) => x.target)).toEqual(['test-device-all--alpha']);
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

import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { NxCompactor, firstFailure, parseNxLog } from '../failures.mjs';

const sample = (name: string) => readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8');

it('takes the failing test of a vitest run, not the summary around it', () => {
  const block = firstFailure(sample('nx-multi-failed.txt'));
  expect(block).toMatch(/^ FAIL {3}nx-host {2}nx\/tests\/zz-tmp.test.ts > tmp fails/);
  expect(block).toContain('AssertionError: expected { a: 1 } to deeply equal { a: 2 }');
  expect(block).not.toContain('Test Files');
});

it('takes the panic of a failed cargo test and skips the earlier warning', () => {
  const block = firstFailure(sample('cargo-test-failure.txt'));
  expect(block).toMatch(/^---- fit::tests::refuses_overlap stdout ----/);
  expect(block).toContain('assertion `left == right` failed');
  expect(block).not.toContain('unused variable');
  expect(block).not.toContain('failures:');
});

it('takes the first rustc error with its snippet, not the could-not-compile line', () => {
  const block = firstFailure(sample('rustc-error.txt'));
  expect(block).toMatch(/^error\[E0308\]: mismatched types/);
  expect(block).toContain('expected `u32`, found `usize`');
  expect(block).not.toContain('could not compile');
});

it('takes the failing pytest case with its assertion', () => {
  const block = firstFailure(sample('pytest-failure.txt'));
  expect(block).toMatch(/^_+ test_blocks_sleep _+/);
  expect(block).toContain('E       AssertionError');
  expect(block).not.toContain('short test summary');
});

it('cuts a long block at the line limit and says how much it dropped', () => {
  const text = `FAIL x > y\n${Array.from({ length: 100 }, (_, i) => `line ${i}`).join('\n')}`;
  const lines = firstFailure(text, 30).split('\n');
  expect(lines).toHaveLength(31);
  expect(lines.at(-1)).toBe('... 71 more lines');
});

it('shows the end of an output that names no failure', () => {
  const text = Array.from({ length: 50 }, (_, i) => `step ${i}`).join('\n');
  const lines = firstFailure(text).split('\n');
  expect(lines[0]).toBe('step 20');
  expect(lines.at(-1)).toBe('step 49');
});

it('reads the tasks of an Nx run: a tick is ok, a section with output is failed', () => {
  const tasks = parseNxLog(sample('nx-multi-failed.txt'));
  expect(tasks.map((t) => [t.id, t.ok])).toEqual([
    ['@digi/dkbs:test-host', true],
    ['nx-tools:test-host', false],
  ]);
  expect(tasks[1].output).toContain('tmp fails');
});

it('prints one line per task, the failure block and a count', () => {
  const printed: string[] = [];
  const filter = new NxCompactor({ print: (l: string) => printed.push(l), logPath: 'out/runs/nx/x.log' });
  const text = sample('nx-multi-failed.txt');
  // split in odd places: the filter works on a stream
  for (let i = 0; i < text.length; i += 37) filter.push(text.slice(i, i + 37));
  filter.end(1);
  const out = printed.join('\n');
  expect(printed[0]).toMatch(
    /^✔ @digi\/dkbs:test-host {2}\[existing outputs match the cache, left as is\] {2}at \d+ s$/,
  );
  expect(out).toMatch(/✖ nx-tools:test-host/);
  expect(out).toContain('full log: out/runs/nx/x.log, section from line');
  expect(out).toContain('  | AssertionError: expected { a: 1 } to deeply equal { a: 2 }');
  expect(out).not.toContain('Run duration');
  expect(printed.at(-1)).toMatch(/^nx: 1 ok, 1 failed, \d+ s; full output out\/runs\/nx\/x.log$/);
});

it('shows what Nx said when no task ran', () => {
  const printed: string[] = [];
  const filter = new NxCompactor({ print: (l: string) => printed.push(l) });
  filter.push(' NX   No tasks were run\n');
  filter.end(0);
  expect(printed[0]).toBe(' NX   No tasks were run');
});

const compact = (text: string, status: number) => {
  const printed: string[] = [];
  const filter = new NxCompactor({ print: (l: string) => printed.push(l) });
  filter.push(text);
  filter.end(status);
  return { printed, tasks: filter.tasks, out: printed.join('\n') };
};

const SECTION = (id: string, note = '') =>
  `> nx run ${id}${note ? `  ${note}` : ''}\n\n> some command\n\nrt.dko.json: unchanged\n\n`;

it('does not fail a task whose output was shown and whose tick never came', () => {
  const text = `✔  nx run a:pack\n${SECTION('core/runtime:build-objects')}✔  nx run b:pack\n\n NX   Successfully ran target x for project y\n`;
  const { tasks, out } = compact(text, 0);
  expect(tasks.map((t) => [t.id, t.ok])).toEqual([
    ['a:pack', true],
    ['core/runtime:build-objects', true],
    ['b:pack', true],
  ]);
  expect(out).toMatch(/nx: 3 ok, 0 failed/);
});

it('starts a section at a header with a cache note and keeps the note', () => {
  const note = '[existing outputs match the cache, left as is]';
  const text = `${SECTION('core/runtime:build-objects')}${SECTION('core/runtime:test-assembly', note)}warning: x\n\n NX   Successfully ran target x\n`;
  const { tasks, out } = compact(text, 0);
  expect(tasks.map((t) => t.id)).toEqual(['core/runtime:build-objects', 'core/runtime:test-assembly']);
  expect(tasks[0].output).not.toContain('warning: x');
  expect(tasks[1].output).toContain('warning: x');
  expect(tasks[1].cache).toBe(note);
  expect(out).toContain(`✔ core/runtime:test-assembly  ${note}`);
});

it('still fails a task with a failure tick or one listed under Failed tasks', () => {
  const tick = compact('✖  nx run a:t\n✔  nx run b:t\n', 1);
  expect(tick.tasks.map((t) => [t.id, t.ok])).toEqual([
    ['a:t', false],
    ['b:t', true],
  ]);
  const listed =
    `${SECTION('good:t')}> nx run bad:t\n\n FAIL  p  f.test.ts > x\nAssertionError: boom\n\n` +
    ' NX   Running target t for 2 projects failed\n\nFailed tasks:\n\n- bad:t\n\n Hint: see log\n';
  const { tasks, out } = compact(listed, 1);
  expect(tasks.map((t) => [t.id, t.ok])).toEqual([
    ['good:t', true],
    ['bad:t', false],
  ]);
  expect(out).toContain('  | AssertionError: boom');
  expect(out).toMatch(/nx: 1 ok, 1 failed/);
});

it('with a non-zero exit and no list, fails the section that holds a failure block', () => {
  const text = `${SECTION('good:t')}> nx run bad:t\n\n FAIL  p  f.test.ts > x\nAssertionError: boom\n\n✔  nx run c:t\n`;
  const { tasks, out } = compact(text, 1);
  expect(tasks.map((t) => [t.id, t.ok])).toEqual([
    ['good:t', true],
    ['bad:t', false],
    ['c:t', true],
  ]);
  expect(out).toContain('  | AssertionError: boom');
});

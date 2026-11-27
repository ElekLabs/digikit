// Compact test output: the first failure block of a task's output, and the filter that turns Nx's
// `static-failures-only` stream into one line per task plus a failure block (used by tools/nx/nx.mjs).
//   node tools/nx/failures.mjs <log file | project:target> [--lines N]
// A log file is a launcher run log (out/runs/nx/*.log), an Nx terminal output (.nx/cache/terminalOutputs/*)
// or any tool output. `project:target` reads the newest launcher run log that has a failed section for it.
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const BLOCK_LINES = 30;
const ANSI = new RegExp(`${String.fromCharCode(27)}\\[[0-9;?]*[A-Za-z]`, 'g');
export const stripAnsi = (text) => String(text).replace(ANSI, '');

// Each detector returns [start, end) line indexes of its first block, or undefined. The block that starts
// first in the output wins: a compiler error comes before the "could not compile" lines that follow it.
const DETECTORS = [
  // vitest: " FAIL  project  file > test name" up to the closing rule of that failure
  (lines) => {
    const start = lines.findIndex((l) => /^\s*FAIL\s+\S/.test(l));
    if (start < 0) return undefined;
    let end = lines.findIndex((l, i) => i > start && l.startsWith('⎯'));
    if (end < 0) end = lines.length;
    return [start, end];
  },
  // cargo test: "---- name stdout ----" up to the "failures:" list
  (lines) => {
    const start = lines.findIndex((l) => /^---- .* stdout ----$/.test(l));
    if (start < 0) return undefined;
    let end = lines.findIndex((l, i) => i > start && /^failures:\s*$/.test(l));
    if (end < 0) end = lines.length;
    return [start, end];
  },
  // rustc: "error[E0308]: ..." or "error: ..." up to the blank line that ends the diagnostic
  (lines) => {
    const start = lines.findIndex(
      (l) =>
        /^error(\[E\d+\])?: /.test(l) && !/could not compile|aborting due to|test failed|process didn't exit/.test(l),
    );
    if (start < 0) return undefined;
    let end = lines.findIndex(
      (l, i) => i > start && l.trim() === '' && /-->|\|/.test(lines.slice(start, i).join('\n')),
    );
    if (end < 0) end = lines.length;
    return [start, end];
  },
  // pytest: "____ test_name ____" up to the next rule
  (lines) => {
    const start = lines.findIndex((l) => /^_{3,} .+ _{3,}$/.test(l));
    if (start < 0) return undefined;
    let end = lines.findIndex((l, i) => i > start && (/^_{3,} .+ _{3,}$/.test(l) || /^={3,}/.test(l)));
    if (end < 0) end = lines.length;
    return [start, end];
  },
  // tsc: "path(1,2): error TS1234: ..." and its indented continuation
  (lines) => {
    const start = lines.findIndex((l) => /error TS\d+:/.test(l));
    if (start < 0) return undefined;
    let end = start + 1;
    while (end < lines.length && /^\s/.test(lines[end])) end++;
    return [start, end];
  },
];

// Only when no detector above found a block: the first line that says it failed
const GENERIC = (lines) => {
  const start = lines.findIndex((l) => /\b(error|failed|fatal|panic(?:ked)?|Traceback)\b/i.test(l));
  return start < 0 ? undefined : [start, lines.length];
};

// The first failure block of `text`, at most `max` lines; the last `max` lines when nothing looks like a failure.
export function firstFailure(text, max = BLOCK_LINES) {
  const lines = stripAnsi(text)
    .split('\n')
    .map((l) => l.replace(/\s+$/, ''));
  while (lines.length && lines.at(-1) === '') lines.pop();
  let best;
  for (const detect of DETECTORS) {
    const found = detect(lines);
    if (found && (!best || found[0] < best[0])) best = found;
  }
  best ??= GENERIC(lines);
  const [start, end] = best ?? [Math.max(0, lines.length - max), lines.length];
  const block = lines.slice(start, Math.min(end, start + max));
  const cut = end - start - block.length;
  return block.join('\n') + (cut > 0 ? `\n... ${cut} more lines` : '');
}

// True when a detector (not the generic fallback) finds a failure block in `text`.
const detectsFailure = (text) => {
  const lines = stripAnsi(text).split('\n');
  return DETECTORS.some((detect) => detect(lines));
};

const TICK = /^(✔|✖|✘|✗|×)\s+nx run (\S+)(?:\s+(\[.*\]))?\s*$/;
const taskId = (id) => id.replace(/^([^:]+:[^:]+):[^:]+$/, '$1');
const HEADER = /^> nx run (\S+)(?:\s+(\[.*\]))?\s*$/;
const FAILED_HEADING = /^Failed tasks:\s*$/;
const FAILED_ITEM = /^- (\S+)\s*$/;
const NX_LINE = /^\s*NX\s/;

// Splits an Nx run output into tasks: { id, ok, cache, lines, output, line } (line: 1-based start in the log).
export function parseNxLog(text) {
  const compactor = new NxCompactor({ print() {}, log: null });
  compactor.push(text);
  compactor.end(0);
  return compactor.tasks;
}

// The filter: feed it the output of `nx ... --output-style=static-failures-only`; it prints one line per task
// as the task finishes and the first failure block of each failed task. `elapsed()` gives seconds since start.
export class NxCompactor {
  constructor({ print, logPath, elapsed = () => 0 }) {
    this.print = print;
    this.logPath = logPath;
    this.elapsed = elapsed;
    this.tasks = [];
    this.rest = '';
    this.seg = undefined;
    this.lineNo = 0;
    this.other = [];
    this.pending = [];
    this.failedIds = new Set();
    this.inList = false;
  }

  push(chunk) {
    this.rest += chunk;
    const lines = this.rest.split('\n');
    this.rest = lines.pop();
    for (const raw of lines) this.line(raw);
  }

  line(raw) {
    this.lineNo++;
    const line = stripAnsi(raw).replace(/\r$/, '');
    const tick = TICK.exec(line);
    const header = HEADER.exec(line);
    if (FAILED_HEADING.test(line)) this.inList = true;
    else if (this.inList) {
      const item = FAILED_ITEM.exec(line);
      if (item) this.failedIds.add(taskId(item[1]));
      else if (line.trim()) this.inList = false;
    }
    if (tick || header || NX_LINE.test(line)) {
      // A task that shows its output has a header and no tick of its own, unless it is this tick.
      if (tick && this.seg?.id === taskId(tick[2])) {
        const seg = this.seg;
        this.seg = undefined;
        return this.finish({ id: seg.id, ok: tick[1] === '✔', cache: tick[3] ?? '', lines: seg.lines, line: seg.line });
      }
      this.close();
    }
    if (tick) {
      if (tick[1] !== '✔') this.failedIds.add(taskId(tick[2]));
      return this.finish({ id: taskId(tick[2]), ok: tick[1] === '✔', cache: tick[3] ?? '', lines: [] });
    }
    if (header) {
      this.seg = { id: taskId(header[1]), cache: header[2] ?? '', lines: [], line: this.lineNo };
      return undefined;
    }
    if (this.seg) this.seg.lines.push(line);
    else if (line.trim()) this.other.push(line);
    return undefined;
  }

  // A section closed by something else than its own tick is undecided: Nx's failure list, or the exit
  // status with a failure block in its output, decides at the end of the run.
  close() {
    if (!this.seg) return;
    const task = { id: this.seg.id, ok: undefined, cache: this.seg.cache, lines: this.seg.lines, line: this.seg.line };
    task.output = task.lines.join('\n');
    this.seg = undefined;
    this.tasks.push(task);
    this.pending.push(task);
  }

  finish(task) {
    task.output ??= task.lines.join('\n');
    if (!this.tasks.includes(task)) this.tasks.push(task);
    const at = Math.round(this.elapsed());
    if (task.ok) this.print(`✔ ${task.id}${task.cache ? `  ${task.cache}` : ''}  at ${at} s`);
    else {
      this.print(`✖ ${task.id}  at ${at} s`);
      if (task.line) this.print(`  full log: ${this.logPath ?? '(none)'}, section from line ${task.line}`);
      const block = task.output ? firstFailure(task.output) : '';
      if (block)
        this.print(
          block
            .split('\n')
            .map((l) => `  | ${l}`)
            .join('\n'),
        );
    }
  }

  end(status) {
    if (this.rest) this.line(this.rest);
    this.rest = '';
    this.close();
    const listed = this.failedIds.size > 0;
    for (const task of this.pending) {
      task.ok = !(listed ? this.failedIds.has(task.id) : status !== 0 && detectsFailure(task.output));
      this.finish(task);
    }
    this.pending = [];
    if (!this.tasks.length) {
      // Nothing ran or Nx stopped before any task (configuration error): show what it said.
      for (const l of this.other.slice(0, 40)) this.print(l);
    } else if (status !== 0 && !this.tasks.some((t) => !t.ok)) {
      for (const l of this.other.slice(-BLOCK_LINES)) this.print(l);
    }
    const failed = this.tasks.filter((t) => !t.ok).length;
    this.print(
      `nx: ${this.tasks.length - failed} ok, ${failed} failed, ${Math.round(this.elapsed())} s${this.logPath ? `; full output ${this.logPath}` : ''}`,
    );
  }
}

// The newest launcher run log with a failed section for `id`.
function logFor(id, dir = 'out/runs/nx') {
  if (!existsSync(dir)) return undefined;
  const files = readdirSync(dir)
    .filter((f) => f.endsWith('.log'))
    .map((f) => join(dir, f))
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  return files.find((f) => parseNxLog(readFileSync(f, 'utf8')).some((t) => t.id === id && !t.ok));
}

export function report(text, { id, max = BLOCK_LINES } = {}) {
  const failed = parseNxLog(text).filter((t) => !t.ok && t.output && (!id || t.id === id));
  if (failed.length) return failed.map((t) => `✖ ${t.id}\n${firstFailure(t.output, max)}`).join('\n\n');
  return firstFailure(text, max);
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const li = args.indexOf('--lines');
  const max = li >= 0 ? Number(args.splice(li, 2)[1]) || BLOCK_LINES : BLOCK_LINES;
  const [target] = args;
  if (!target) {
    console.error('usage: node tools/nx/failures.mjs <log file | project:target> [--lines N]');
    process.exit(2);
  }
  if (existsSync(target)) console.log(report(readFileSync(target, 'utf8'), { max }));
  else {
    const file = logFor(target);
    if (!file) {
      console.error(`failures: no file ${target} and no failed section of it in out/runs/nx/*.log`);
      process.exit(2);
    }
    console.log(`${file}\n${report(readFileSync(file, 'utf8'), { id: target, max })}`);
  }
}

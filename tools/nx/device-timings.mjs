// Measured run seconds of the device test halves of one gate run.
//   node tools/nx/device-timings.mjs [LOG] [--durations DIR] [--md] [--top N] [--audit audit.md]
// LOG is a launcher run log (out/runs/nx/*.log; default: the newest). Its `nx run` lines name the tasks of the run, their
// cache status and the run duration. The seconds come from tools/devtools/emu-slot.mjs, which records the seconds a passed
// task held its emulator slot (slot wait excluded) in `<git common dir>/emu-slots/durations/`: one file per target,
// overwritten by the next passed run, so a file whose mtime lies outside the run window is marked `*` (another run).
// Project, test file and DSP mode (coupled = DIGI_EMU_DSP sharc) come from the Nx project graph (.nx/workspace-data).
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { stripAnsi } from './failures.mjs';

export const durationName = (id) => id.replace(/[^\w.-]/g, '_'); // emu-slot.mjs durationFile

// The tasks of a run log: id (maybe cut at the left by Nx with "..."), status and the run window.
export const parseRunLog = (text, name = '') => {
  const lines = stripAnsi(text).split('\n');
  const tasks = [];
  for (const line of lines) {
    // "✔  nx run ID  [cache status]"; Nx cuts a long line at the left ("...ID  [existing outputs match the cache, l...")
    const m = /^(?:([✔✖])\s+nx run )?(\.{3}\S+|[\w@./-]+:[\w.-]+)(?:\s+\[(.*))?$/.exec(line.trimEnd());
    if (m && (m[1] || m[2].startsWith('...')))
      tasks.push({ id: m[2], cached: m[3] !== undefined && /cache/.test(m[3]), failed: m[1] === '✖' });
  }
  const failed = new Set();
  const at = lines.indexOf('Failed tasks:');
  if (at >= 0)
    for (const l of lines.slice(at + 1))
      if (l.startsWith('- ')) failed.add(l.slice(2).trim());
      else if (l.startsWith(' NX')) break;
  const dur = /Run duration:\s+(?:(\d+)m )?(\d+(?:\.\d+)?)s/.exec(stripAnsi(text));
  const wall = dur ? Number(dur[1] ?? 0) * 60 + Number(dur[2]) : undefined;
  const stamp = /^(\d{4}-\d\d-\d\d)T(\d\d)-(\d\d)-(\d\d)-(\d+)Z/.exec(name);
  const start = stamp ? Date.parse(`${stamp[1]}T${stamp[2]}:${stamp[3]}:${stamp[4]}.${stamp[5]}Z`) : undefined;
  return {
    tasks,
    failed: [...failed],
    wall,
    start,
    end: start !== undefined && wall !== undefined ? start + wall * 1000 : undefined,
  };
};

// Task ids of a device test target: `project:test-device...`; a cut id ("...e/fx:test-device--half") resolves by suffix.
export const resolveId = (id, known) => {
  if (!id.startsWith('...')) return id;
  const tail = id.slice(3);
  const hits = known.filter((k) => k.endsWith(tail));
  return hits.length === 1 ? hits[0] : id;
};

// Facts of a target from the project graph node: its test files and DSP mode.
export const targetFacts = (target) => {
  const cmd = target?.options?.command ?? '';
  const files = [...cmd.matchAll(/(\S+\.device\.test\.ts)/g)].map((m) => m[1].replace(/^(\.\.\/)+/, ''));
  return { files, mode: target?.options?.env?.DIGI_EMU_DSP === 'sharc' ? 'coupled' : 'zero' };
};

export const buildRows = (run, graph, durDir) => {
  const nodes = graph.nodes ?? {};
  const known = Object.entries(nodes).flatMap(([p, n]) => Object.keys(n.data?.targets ?? {}).map((t) => `${p}:${t}`));
  const rows = [];
  for (const t of run.tasks) {
    const id = resolveId(t.id, known);
    const cut = id.indexOf(':');
    const project = id.slice(0, cut);
    const target = id.slice(cut + 1);
    if (!/^test-device(-all)?(--|$)/.test(target)) continue;
    const facts = targetFacts(nodes[project]?.data?.targets?.[target]);
    const file = durDir && join(durDir, durationName(id));
    let seconds;
    let outside = false;
    let unverified = false;
    if (file && existsSync(file)) {
      seconds = Number(readFileSync(file, 'utf8'));
      const m = statSync(file).mtimeMs;
      if (Number.isFinite(run.end)) outside = m < run.start - 60000 || m > run.end + 60000;
      else unverified = true; // run window unknown: the file may belong to any run
    }
    const half = /--([^-]+)$/.exec(target)?.[1];
    // the plain `test-device` parent runs nothing: it carries no files
    if (!facts.files.length && seconds === undefined) continue;
    rows.push({
      id,
      project,
      target,
      device: half ?? '',
      files: facts.files,
      mode: facts.mode,
      seconds,
      cached: t.cached,
      failed: t.failed || run.failed.includes(id),
      outside,
      unverified,
    });
  }
  return rows;
};

const fmt = (s) => (s === undefined ? 'n/a' : s >= 60 ? `${(s / 60).toFixed(1)} min` : `${s.toFixed(0)} s`);
const base = (f) => f.replace(/^.*\//, '').replace(/\.device\.test\.ts$/, '');

// Rows whose seconds do not belong to this run: failed (no new record), written outside the run window, or window unknown.
export const stale = (r) => r.failed || r.outside || r.unverified;

export const summarize = (rows, run) => {
  const ran = rows.filter((r) => !r.cached && r.seconds !== undefined && !stale(r));
  const sum = ran.reduce((a, r) => a + r.seconds, 0);
  return {
    ran,
    sum,
    byMode: (m) => ran.filter((r) => r.mode === m).reduce((a, r) => a + r.seconds, 0),
    wall: run.wall,
  };
};

export const table = (rows, { top = 20 } = {}) => {
  const sorted = [...rows].sort((a, b) => (b.seconds ?? -1) - (a.seconds ?? -1));
  const line = (r) =>
    `| ${r.device || '-'} | ${fmt(r.seconds)}${r.outside ? ' *' : ''}${r.unverified ? ' ?' : ''} | ${r.mode} | ${r.project} | ${r.files.map(base).join(', ') || r.target} | ${r.cached ? 'cached' : r.failed ? 'failed' : 'ran'} |`;
  const head = '| half | run time | mode | project | file | this run |\n|---|---|---|---|---|---|';
  return {
    all: `${head}\n${sorted.map(line).join('\n')}`,
    slowest: `${head}\n${sorted.slice(0, top).map(line).join('\n')}`,
  };
};

// Minutes per file of a device suite audit table (`| file | project | halves | min before | ...`, out/runs/device-suite/audit.md).
export const parseAudit = (text) => {
  const out = new Map();
  for (const l of text.split('\n')) {
    const c = l.split('|').map((x) => x.trim());
    if (c.length > 5 && /^[\w-]+$/.test(c[1]) && /^\d+(\.\d+)?$/.test(c[4])) out.set(c[1], Number(c[4]));
  }
  return out;
};

// Per test file: measured minutes summed over its halves, next to the audit's; `factor` (default 2) marks a larger gap.
export const compareAudit = (rows, audit, factor = 2) => {
  const per = new Map();
  for (const r of rows)
    if (r.files.length === 1 && r.seconds !== undefined && !r.cached && !stale(r))
      per.set(base(r.files[0]), (per.get(base(r.files[0])) ?? 0) + r.seconds / 60);
  const lines = [];
  for (const [file, min] of [...per].sort((a, b) => b[1] - a[1])) {
    const was = audit.get(file);
    if (was === undefined) continue;
    const ratio = Math.max(was / min, min / was);
    if (ratio > factor) lines.push(`| ${file} | ${min.toFixed(1)} | ${was.toFixed(1)} | ${ratio.toFixed(1)}x |`);
  }
  return `| file | measured min (halves summed) | audit min | gap |\n|---|---|---|---|\n${lines.join('\n')}`;
};

const newestLog = (dir) => {
  const logs = readdirSync(dir)
    .filter((f) => f.endsWith('.log'))
    .sort();
  return logs.length ? join(dir, logs[logs.length - 1]) : undefined;
};

const main = () => {
  const args = process.argv.slice(2);
  const opt = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const logArg = args.find((a, i) => !a.startsWith('--') && !['--durations', '--top', '--audit'].includes(args[i - 1]));
  const logFile = logArg ?? newestLog('out/runs/nx');
  if (!logFile) throw new Error('no run log: pass one (out/runs/nx/*.log)');
  const common = spawnSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], {
    encoding: 'utf8',
  }).stdout.trim();
  const durDir = opt('--durations') ?? join(common, 'emu-slots', 'durations');
  const graphFile = '.nx/workspace-data/project-graph.json';
  if (!existsSync(graphFile)) throw new Error(`${graphFile} missing: run any pnpm nx:pilot command first`);
  const run = parseRunLog(readFileSync(logFile, 'utf8'), logFile.replace(/^.*\//, ''));
  const rows = buildRows(run, JSON.parse(readFileSync(graphFile, 'utf8')), durDir);
  const s = summarize(rows, run);
  const t = table(rows, { top: Number(opt('--top') ?? 20) });
  console.log(
    `log ${logFile}: ${run.tasks.length} tasks, ${rows.length} device halves (${rows.filter((r) => r.cached).length} cached)`,
  );
  console.log(
    `summed run time of ${s.ran.length} halves that ran: ${fmt(s.sum)} (coupled ${fmt(s.byMode('coupled'))}, zero-DSP ${fmt(s.byMode('zero'))}); run wall ${fmt(s.wall)}`,
  );
  if (rows.some((r) => r.unverified))
    console.warn(
      'WARNING: the run window is unknown (log name without a timestamp, or no "Run duration": a killed run): every row is unverified (`?`) and left out of the sums',
    );
  console.log(
    '`*` = the duration file was written outside this run window (a later or earlier passed run); `?` = run window unknown; both and failed rows are left out of the sums\n',
  );
  console.log(args.includes('--md') ? t.all : t.slowest);
  const auditFile = opt('--audit');
  if (auditFile)
    console.log(
      `\nFiles whose audit minutes differ by more than 2x:\n\n${compareAudit(rows, parseAudit(readFileSync(auditFile, 'utf8')))}`,
    );
};

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) main();

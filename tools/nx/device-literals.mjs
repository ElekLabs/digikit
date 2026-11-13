// Device-literal check: tracked files may not name a device ('dt2', 'dn2', 'digitakt', 'digitone', taken from the registry in
// devices/*.toml) outside the places that are allowed to. The files that name one today are listed with their counts in
// device-literals.baseline.json, and the check fails on ANY difference from it: a file that gains literals or appears, and a file
// that loses them (the author commits the smaller baseline in the same change). `node tools/nx/device-literals.mjs --write`
// rewrites the baseline; `--report` prints the totals and the top 20.
//
// Only tracked files are scanned (`git ls-files`, staged new files included), so a local untracked file never changes the result.
// Binary files (a NUL byte) are skipped and counted.
//
// Allowed without a baseline entry:
//   - devices/** and docs/**;
//   - a path with a segment that is, or has a separator-bounded token equal to, a device id or name word (`dt2/`, `device-dt2.inc`,
//     `foo_dn2.py`, `digitakt/`): per-device adapter sources, single-device RE tools, tests pinned to one device. `digitakt2/` and
//     `dt2fx.py` are not such tokens;
//   - a file with the marker `device-literals: allow` on a comment line of its own in its first 5 lines;
//   - in a .toml file: table headers (`[name]`, `[[name]]`) and the whole value of a `ports =` or `devices =` key, continuation
//     lines of a multi-line array included.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadDevices } from '../../packages/devices/src/node.ts';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const BASELINE = join(root, 'tools/nx/device-literals.baseline.json');
// never scanned: this check's own files and lock files
const SKIP = new Set([
  'tools/nx/device-literals.mjs',
  'tools/nx/device-literals.baseline.json',
  'pnpm-lock.yaml',
  'Cargo.lock',
  'uv.lock',
]);

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const BOUND = '(?<![a-z0-9])';
const END = '(?![a-z0-9])';

/** The literals of the registry: each short id and the first word of each name, bounded by non-alphanumerics (in text and in paths). */
export function literalPatterns(registry) {
  const ids = registry.devices.map((d) => escape(d.id));
  const words = [...new Set(registry.devices.map((d) => escape(d.name.split(/\s+/)[0].toLowerCase())))];
  const one = new RegExp(`${BOUND}(?:${[...ids, ...words].join('|')})${END}`, 'i');
  return { text: new RegExp(one.source, 'gi'), path: one };
}

const hits = (line, re) => line.match(re)?.length ?? 0;
const hasMarker = (text) =>
  text
    .split('\n')
    .slice(0, 5)
    .some((l) => /^\s*(\/\/|#|\/\*|<!--|--)\s*device-literals: allow\b/.test(l));

/** The number of literals in `text` that no rule allows. */
export function countLiterals(path, text, re) {
  if (hasMarker(text)) return 0;
  const toml = path.endsWith('.toml');
  let n = 0;
  let inList = false;
  for (const line of text.split('\n')) {
    if (toml) {
      if (inList) {
        // continuation of a `ports =` / `devices =` array, up to the closing bracket
        if (line.includes(']')) inList = false;
        continue;
      }
      if (/^\s*\[\[?[A-Za-z0-9_.\-"' ]+\]\]?\s*(#.*)?$/.test(line)) continue;
      const m = /^\s*(ports|devices)\s*=\s*(.*)$/.exec(line);
      if (m) {
        if (m[2].startsWith('[') && !m[2].includes(']')) inList = true;
        continue;
      }
    }
    n += hits(line, re);
  }
  return n;
}

export function allowedPath(path, pathRe) {
  return path.startsWith('devices/') || path.startsWith('docs/') || pathRe.test(path);
}

/** `found`: path to count, for the files `readText` can read as text (it returns undefined for a binary file). */
export function scan(files, readText, patterns) {
  const found = {};
  let binary = 0;
  for (const path of files) {
    if (SKIP.has(path) || allowedPath(path, patterns.path)) continue;
    const text = readText(path);
    if (text === undefined) {
      binary++;
      continue;
    }
    const n = countLiterals(path, text, patterns.text);
    if (n > 0) found[path] = n;
  }
  return { found, binary };
}

/** `found` against `baseline`: what grew (or is new) and what shrank (or is gone). Any difference fails. */
export function compare(found, baseline) {
  const grew = [];
  const shrank = [];
  for (const [path, n] of Object.entries(found)) {
    const was = baseline[path] ?? 0;
    if (n > was) grew.push({ path, was, now: n });
    else if (n < was) shrank.push({ path, was, now: n });
  }
  for (const [path, was] of Object.entries(baseline)) if (!(path in found)) shrank.push({ path, was, now: 0 });
  return { grew, shrank };
}

function readText(path) {
  let buf;
  try {
    buf = readFileSync(join(root, path));
  } catch {
    return ''; // listed but gone from the work tree (a deletion not yet staged): nothing to count
  }
  return buf.includes(0) ? undefined : buf.toString('utf8');
}

function main() {
  const args = new Set(process.argv.slice(2));
  const files = execFileSync('git', ['ls-files', '-z'], { cwd: root, maxBuffer: 1 << 28 })
    .toString()
    .split('\0')
    .filter(Boolean);
  const { found, binary } = scan(files, readText, literalPatterns(loadDevices()));
  const total = Object.values(found).reduce((a, b) => a + b, 0);
  console.log(
    `device-literals: ${total} literals in ${Object.keys(found).length} files (${binary} binary files skipped)`,
  );
  if (args.has('--write')) {
    const sorted = Object.fromEntries(Object.entries(found).sort(([a], [b]) => (a < b ? -1 : 1)));
    writeFileSync(BASELINE, JSON.stringify(sorted, null, 2) + '\n');
    console.log('device-literals: baseline written');
    return;
  }
  if (args.has('--report'))
    for (const [p, n] of Object.entries(found)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 20))
      console.log(`${String(n).padStart(6)}  ${p}`);
  const baseline = JSON.parse(readFileSync(BASELINE, 'utf8'));
  const { grew, shrank } = compare(found, baseline);
  if (grew.length + shrank.length > 0) {
    console.error('device-literals: the files that name a device differ from tools/nx/device-literals.baseline.json:');
    for (const g of [...grew, ...shrank]) console.error(`  ${g.path}: ${g.was} -> ${g.now}`);
    console.error(
      grew.length > 0
        ? 'Take the device from the registry (@digi/devices, devices/*.toml) instead of naming it.'
        : 'Fewer literals is good: commit the smaller baseline in this change.',
    );
    console.error(
      'When the difference is intended, run `node tools/nx/device-literals.mjs --write` and commit the baseline.',
    );
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();

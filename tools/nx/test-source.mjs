// What counts as a unit-test source: it holds tests, not the product, so an edit of it selects no device or conformance test.
// The rule is data, config/test-source-patterns.json, and this file is its one TS reader: it derives the Nx globs and the name
// test, and it owns the cfg(test) guard. The other readers are tools/devtools/emud_key.py and
// packages/patchkit/src/build/test_source.rs. The `production` input of nx.json is static JSON and is checked against `testSourceGlobs`
// by tools/nx/tests/test-source.test.ts.
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TEST_SOURCES = JSON.parse(
  readFileSync(fileURLToPath(new URL('../../config/test-source-patterns.json', import.meta.url)), 'utf8'),
);

// A name pattern with at most one `*`.
const nameMatches = (pattern, name) => {
  const [head, tail] = pattern.split('*');
  return tail === undefined
    ? name === head
    : name.length >= head.length + tail.length && name.startsWith(head) && name.endsWith(tail);
};

// `rel`: a path inside a project, with `/`.
export const isTestSource = (rel) => {
  const name = basename(rel);
  return (
    dirname(rel)
      .split('/')
      .some((d) => TEST_SOURCES.dirs.includes(d)) ||
    (name.endsWith('.rs') && TEST_SOURCES.rust_files.some((p) => nameMatches(p, name)))
  );
};

/** The exclusion globs of the rule below `base` (for example `!{projectRoot}`): one per test directory, one for the file names.
 * `ext` narrows the directory globs to a file extension (`.rs`); the name glob is Rust only. */
export const testSourceGlobs = (base, ext = '') => [
  ...TEST_SOURCES.dirs.map((d) => `!${base}/**/${d}/**/*${ext}`),
  `!${base}/**/{${TEST_SOURCES.rust_files.join(',')}}`,
];

// The line that declares `mod <name>;` and the attribute lines right above it (comments are not attributes).
const declarations = (text, name) => {
  const lines = text.split('\n').map((l) => l.trim());
  const decl = new RegExp(`^(?:pub(?:\\([a-z]+\\))? )?mod ${name};`);
  return lines.flatMap((line, i) => {
    if (!decl.test(line)) return [];
    const attrs = [];
    for (let j = i - 1; j >= 0 && (lines[j].startsWith('#[') || lines[j].startsWith('//')); j--)
      if (lines[j].startsWith('#[')) attrs.push(lines[j]);
    return [attrs];
  });
};
const CFG_TEST = /^#!?\[cfg\((?:all\()?test\b/;

/** The Rust test sources of the workspace (relative names) that a release build could compile in: a name that matches the rule
 * and is neither declared as `mod x;` behind `#[cfg(test)]` (or `cfg(all(test, ...))`) in a sibling or parent file, nor a
 * file with an inner `#![cfg(test)]`. `files` are the Rust files to check (relative, `/`-separated); files below a test directory
 * are integration tests and no modules. */
export const unguardedTestSources = (workspaceRoot, files) => {
  const inTestDir = (rel) =>
    rel
      .split('/')
      .slice(0, -1)
      .some((d) => TEST_SOURCES.dirs.includes(d));
  const guarded = (rel) => {
    const text = readFileSync(join(workspaceRoot, rel), 'utf8');
    if (text.split('\n').some((l) => l.trim().startsWith('#![') && CFG_TEST.test(l.trim()))) return true;
    const own = join(workspaceRoot, dirname(rel));
    const name = basename(rel, '.rs');
    const parents = [own, dirname(own)].flatMap((d) =>
      readdirSync(d)
        .filter((f) => f.endsWith('.rs') && join(d, f) !== join(workspaceRoot, rel))
        .map((f) => join(d, f)),
    );
    const found = parents.flatMap((p) => declarations(readFileSync(p, 'utf8'), name));
    return found.length > 0 && found.every((attrs) => attrs.some((a) => CFG_TEST.test(a)));
  };
  return files.filter((rel) => rel.endsWith('.rs') && !inTestDir(rel) && isTestSource(rel) && !guarded(rel)).sort();
};

/** The Rust files git tracks below `workspaceRoot`: the same set the Nx input `**\/*.rs` sees (ignored trees left out). */
export const trackedRustFiles = (workspaceRoot) =>
  execFileSync('git', ['ls-files', '-z', '--', '*.rs'], { cwd: workspaceRoot, encoding: 'utf8' })
    .split('\0')
    .filter(Boolean);

// CLI (the `test-source` target of the workspace project): list the unguarded Rust test sources and fail.
if (process.argv[1] === fileURLToPath(import.meta.url) && process.argv.includes('--check')) {
  const root = fileURLToPath(new URL('../..', import.meta.url));
  const bad = unguardedTestSources(root, trackedRustFiles(root));
  if (bad.length > 0) {
    console.error('test-source: Rust test sources not declared behind #[cfg(test)] (the keys hide them):');
    for (const f of bad) console.error(`  ${f}`);
    process.exit(1);
  }
  console.log('test-source: every Rust test source is behind cfg(test)');
}

import { afterAll, expect, it } from 'vitest';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
// @ts-expect-error plain module
import { isTestSource, testSourceGlobs, unguardedTestSources, TEST_SOURCES } from '../test-source.mjs';
// @ts-expect-error plain module
import { subjectInputs } from '../device-test-keys.mjs';

const root = join(__dirname, '../../..');
const sample = (pattern: string) => pattern.replace('*', 'x');
const tests = [
  ...TEST_SOURCES.dirs.flatMap((d: string) => [`${d}/x.rs`, `src/a/${d}/y/z.rs`, `${d}/data.json`]),
  ...TEST_SOURCES.rust_files.map((p: string) => `src/a/${sample(p)}`),
];
const product = [
  'src/lib.rs',
  'src/loader/install.rs',
  'src/contest.rs',
  'src/test_lock.rs',
  'module.toml',
  'src/tests.txt',
];

it('names the test sources of the rule, and only those', () => {
  for (const f of tests) expect(isTestSource(f), f).toBe(true);
  for (const f of product) expect(isTestSource(f), f).toBe(false);
});

// The static `production` input of nx.json cannot import the rule: its exclusions are the derived list, nothing missing, nothing extra.
it('holds in nx.json the exclusions the rule derives', () => {
  const production: string[] = JSON.parse(readFileSync(join(root, 'nx.json'), 'utf8')).namedInputs.production;
  const excluded = production.filter((g) => g.startsWith('!'));
  expect(new Set(excluded)).toEqual(new Set(['!{projectRoot}/**/*.md', ...testSourceGlobs('{projectRoot}')]));
  expect(excluded.length).toBe(new Set(excluded).size);
});

it('holds in subjectInputs the exclusions the rule derives', () => {
  const globs: string[] = subjectInputs('modules/x', root);
  const mine = globs.filter((g) => g.startsWith('!') && g.includes('/**/') && !g.endsWith('.md'));
  expect(new Set(mine)).toEqual(new Set(testSourceGlobs('{workspaceRoot}/modules/x')));
});

const temp = mkdtempSync(join(tmpdir(), 'test-source-'));
afterAll(() => rmSync(temp, { recursive: true, force: true }));
const tree = (name: string, files: Record<string, string>) => {
  const dir = join(temp, name);
  for (const [f, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    writeFileSync(join(dir, f), text);
  }
  return dir;
};
// The guard over the files of a temporary tree, as the CLI runs it over the files git tracks.
const check = (name: string, files: Record<string, string>) =>
  unguardedTestSources(tree(name, files), Object.keys(files));

it('flags a test source that a release build could compile in', () => {
  expect(check('bare', { 'c/src/lib.rs': 'mod tests;\n', 'c/src/tests.rs': '' })).toEqual(['c/src/tests.rs']);
  // a cfg(test) in a comment or on another line is no guard
  expect(
    check('commented', {
      'c/src/lib.rs': '// #[cfg(test)]\nmod tests;\n',
      'c/src/tests.rs': '',
    }),
  ).toEqual(['c/src/tests.rs']);
  expect(check('undeclared', { 'c/src/lib.rs': '', 'c/src/fake_x.rs': '' })).toEqual(['c/src/fake_x.rs']);
  expect(check('other', { 'c/src/lib.rs': '#[cfg(feature = "a")]\nmod a_tests;\n', 'c/src/a_tests.rs': '' })).toEqual([
    'c/src/a_tests.rs',
  ]);
});

it('passes a test source behind cfg(test), in any accepted form', () => {
  expect(
    check('guarded', {
      'c/src/lib.rs': '#[cfg(test)]\nmod tests;\n#[cfg(all(test, feature = "x"))]\npub(crate) mod fake_y;\n',
      'c/src/tests.rs': '',
      'c/src/fake_y.rs': '',
      'c/src/inner.rs': '#![cfg(test)]\n',
      'c/src/tests_inner.rs': '#![cfg(test)]\nfn t() {}\n',
      'c/src/a/mod.rs': '#[cfg(test)]\n#[allow(dead_code)]\nmod x_tests;\n',
      'c/src/a/x_tests.rs': '',
      'c/tests/integration.rs': 'fn main() {}\n', // below a test directory: no module
    }),
  ).toEqual([]);
});

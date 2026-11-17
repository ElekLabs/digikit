import { expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { baseProblem, changedFiles, invocations, resolveSubset, selectionPairs, sharedInput } from '../test-subset.mjs';

const project = (name: string, targets: string[], runtimeAcceptance: object[]) => ({
  name,
  targets,
  metadata: { runtimeAcceptance },
});
const projects = [
  // two files: per-file leaves
  project(
    'core/a',
    ['test-device', 'test-device--x', 'test-device--x--dt2', 'test-device--x--dn2', 'test-device--y--dn2'],
    [
      { files: ['tests/x.device.test.ts'], tags: ['S:machines', 'C:loading'] },
      { files: ['tests/y.device.test.ts'], tags: ['C:boot'] },
    ],
  ),
  // one file: the leaves have no file part
  project(
    'core/b',
    ['test-device', 'test-device--dt2', 'test-device--dn2'],
    [{ files: ['b.device.test.ts'], tags: ['S:machines'] }],
  ),
  project('gate-smoke', ['smoke'], []),
  project('gate-pins', ['pins'], []),
];

it('resolves tags to the leaf targets of both devices, as a union', () => {
  const { tests, unknown } = resolveSubset(['S:machines', 'C:boot'], projects);
  expect(unknown).toEqual([]);
  expect(tests.map((t) => [t.project, t.targets])).toEqual([
    ['core/a', ['test-device--x--dt2', 'test-device--x--dn2']],
    ['core/a', ['test-device--y--dn2']],
    ['core/b', ['test-device--dt2', 'test-device--dn2']],
  ]);
});

it('reports a tag that no record has', () => {
  expect(resolveSubset(['S:fx', 'C:boot'], projects).unknown).toEqual(['S:fx']);
});

it('adds gate-smoke and gate-pins, and runs one call when the cross product is exact', () => {
  const { tests } = resolveSubset(['C:boot'], projects);
  const pairs = selectionPairs(tests, projects);
  expect(pairs.map((p) => `${p.project}:${p.target}`)).toEqual([
    'core/a:test-device--y--dn2',
    'gate-smoke:smoke',
    'gate-pins:pins',
  ]);
  expect(invocations(pairs, projects)).toEqual([
    ['run-many', '-t', 'test-device--y--dn2,smoke,pins', '-p', 'core/a,gate-smoke,gate-pins'],
  ]);
});

it('splits the calls when a project would run a target that was not selected', () => {
  const both = [
    ...projects,
    project('core/c', ['test-device--dn2'], [{ files: ['tests/c.device.test.ts'], tags: ['C:state'] }]),
  ];
  const pairs = [
    { project: 'core/b', target: 'test-device--dt2' },
    { project: 'core/c', target: 'test-device--dn2' },
    { project: 'core/b', target: 'test-device--dn2' },
  ];
  expect(invocations(pairs.slice(0, 2), both)).toEqual([
    ['run-many', '-t', 'test-device--dt2', '-p', 'core/b'],
    ['run-many', '-t', 'test-device--dn2', '-p', 'core/c'],
  ]);
});

const graph = [
  {
    ...project(
      'core/a',
      ['test-device--dt2'],
      [{ files: ['tests/x.device.test.ts'], tags: ['C:boot'], modules: ['core/b'] }],
    ),
    root: 'modules/a',
  },
  {
    ...project('core/b', ['test-device--dt2'], [{ files: ['b.device.test.ts'], tags: ['C:state'], dsp: 'sharc' }]),
    root: 'modules/b',
  },
  {
    ...project('core/c', ['test-device--dt2'], [{ files: ['tests/c.device.test.ts'], tags: ['C:tick'] }]),
    root: 'modules/c',
  },
  { ...project('patchkit', [], []), root: 'packages/patchkit' },
];
const names = (changed: string[], extra: object = {}) =>
  resolveSubset([], graph, { changed, ...extra })
    .tests.map((t) => t.project)
    .sort();

it('selects the records whose subject changed since a base, beside the tags', () => {
  // a change in core/b reaches the record that loads it; a project.json or a test change of another project does not
  expect(names(['modules/b/src/x.c'])).toEqual(['core/a', 'core/b']);
  expect(names(['modules/c/project.json', 'modules/c/tests/other.device.test.ts', 'docs/x.md'])).toEqual([]);
  expect(names(['modules/c/tests/c.device.test.ts'])).toEqual(['core/c']);
  expect(
    resolveSubset([], graph, { changed: ['modules/b/src/x.c'] }).tests.find((t) => t.project === 'core/b')?.dsp,
  ).toBe('sharc');
});

it('selects the records whose test files import a changed helper', () => {
  const closureOf = (root: string, file: string) => [
    `${root}/${file}`,
    ...(root === 'modules/c' ? ['tests/helper/kit.ts'] : []),
  ];
  expect(names(['tests/helper/kit.ts'], { closureOf })).toEqual(['core/c']);
  expect(names(['tests/helper/other.ts'], { closureOf })).toEqual([]);
});

it('errors on a module id that is no project', () => {
  const bad = [
    { ...project('core/a', [], [{ files: ['a.device.test.ts'], modules: ['no/such'] }]), root: 'modules/a' },
  ];
  expect(() => resolveSubset([], bad, { changed: ['x'] })).toThrow(/no\/such .* is no Nx project/);
});

it('reports a shared input, and refuses an empty or shared selection without tags', () => {
  expect(sharedInput('sdk/sdk.toml', graph)).toMatch(/shared key input/);
  expect(sharedInput('packages/patchkit/src/main.rs', graph)).toBe('the source of patchkit');
  expect(sharedInput('modules/c/src/x.c', graph)).toBe('');
  const sharedOf = (f: string) => sharedInput(f, graph);
  const run = (changed: string[], tags: string[] = []) => {
    const got = resolveSubset(tags, graph, { changed, sharedOf });
    return baseProblem({ tags, base: 'B', changed, tests: got.tests, shared: got.shared });
  };
  expect(run(['packages/patchkit/src/main.rs', 'modules/b/src/x.c'])).toMatch(/source of patchkit.*name tags/i);
  expect(run(['docs/x.md'])).toMatch(/select no device test record.*name tags/i);
  expect(run([])).toMatch(/no file changed/);
  expect(run(['modules/b/src/x.c'])).toBe('');
  expect(run(['docs/x.md'], ['C:boot'])).toBe('');
});

it('counts an untracked file as changed', () => {
  const dir = mkdtempSync(join(tmpdir(), 'subset-git-'));
  try {
    const git = (...a: string[]) => execFileSync('git', ['-C', dir, ...a], { encoding: 'utf8' });
    git('init', '-q');
    writeFileSync(join(dir, 'a.txt'), 'a');
    git('add', 'a.txt');
    git('-c', 'user.email=a@b', '-c', 'user.name=n', 'commit', '-q', '-m', 'x');
    writeFileSync(join(dir, 'new.ts'), 'b');
    writeFileSync(join(dir, 'a.txt'), 'c');
    expect(changedFiles(dir, 'HEAD').sort()).toEqual(['a.txt', 'new.ts']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

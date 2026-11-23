import { expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { tagProblem } from '../acceptance-tags.mjs';
import { parseOwners } from '../area-paths.mjs';
import { TIERS, deviceByArea } from '../gate.mjs';
import {
  DEVICES,
  DEVICE_SMOKE,
  affectedFixed,
  areaSelection,
  conformanceSelection,
  halvesOfDevices,
  baseProblem,
  changedFiles,
  deviceHalves,
  invocations,
  projectsWithTarget,
  resolveSubset,
  selectionPairs,
  skipText,
  skippedHalves,
  smokeHalves,
  whyText,
} from '../test-subset.mjs';

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

it('adds gate-smoke and gate-pins only when affected selects them', () => {
  const none = affectedFixed('main', () => []);
  expect(none).toEqual([]);
  const { tests } = resolveSubset(['C:boot'], projects);
  expect(selectionPairs(tests, projects, none).map((p) => `${p.project}:${p.target}`)).toEqual([
    'core/a:test-device--y--dn2',
  ]);
  const only = affectedFixed('main', (_base, target) => (target === 'smoke' ? ['gate-smoke'] : []));
  expect(only.map((f) => f.project)).toEqual(['gate-smoke']);
});

it('runs one call when the cross product is exact', () => {
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

it('knows the mechanism tags and refuses a misspelt one', () => {
  const none = new Map();
  for (const tag of ['C:device-facts', 'C:fault', 'C:types']) expect(tagProblem(tag, none)).toBe('');
  expect(tagProblem('C:fualt', none)).toMatch(/names no area/);
});

it('says per selected file which given tags and which changed files selected it', () => {
  const mech = [
    {
      ...project('core/m', projects[1]!.targets, [
        { files: ['tests/f.device.test.ts'], tags: ['C:fault', 'C:diagnostics'] },
        { files: ['tests/t.device.test.ts'], tags: ['C:types'] },
      ]),
      root: 'modules/m',
    },
  ];
  // only the given tags of a file are named, not all its tags
  const byTag = resolveSubset(['C:fault', 'C:types'], mech).tests;
  expect(byTag.map((t) => [t.file, whyText(t)])).toEqual([
    ['tests/f.device.test.ts', 'C:fault'],
    ['tests/t.device.test.ts', 'C:types'],
  ]);
  // a tag and a changed source file of the same record both show; a record selected by the change alone names the file
  const both = resolveSubset(['C:types'], mech, { changed: ['modules/m/src/a.c'] }).tests;
  expect(both.map((t) => [t.file, whyText(t)])).toEqual([
    ['tests/f.device.test.ts', 'changed modules/m/src/a.c'],
    ['tests/t.device.test.ts', 'C:types, changed modules/m/src/a.c'],
  ]);
  const long = whyText({ tags: [], changed: ['a', 'b', 'c', 'd', 'e'] });
  expect(long).toBe('changed a, changed b, changed c, 2 more changed');
});

const [d1, d2, d3] = DEVICES;
// The area gate: changed paths name areas, the tests of those areas run, the smoke set is fixed, the rest is skipped.
const areaGraph = [
  { ...project('core/runtime', [], []), root: 'runtime/core' },
  {
    ...project(
      'core/rt-tests',
      [
        `test-device--boot--${d1}`,
        `test-device--boot--${d2}`,
        `test-device--storage--${d1}`,
        `test-device--storage--${d2}`,
      ],
      [
        { files: ['boot.device.test.ts'], tags: ['C:boot'] },
        { files: ['storage.device.test.ts'], tags: ['C:storage'] },
      ],
    ),
    root: 'runtime/core/tests',
    subjects: ['core/runtime'],
  },
  {
    ...project('core/m', [`test-device--${d1}`], [{ files: ['tests/m.device.test.ts'], tags: ['C:state'] }]),
    root: 'modules/m',
  },
  { ...project('gate-device-smoke', ['device'], []), root: 'tools/gate/device-smoke' },
  { ...project('patchkit', [], []), root: 'packages/patchkit' },
  { ...project('core/m', ['test-conformance'], []), root: 'modules/m' },
  { ...project('core/other', ['test-conformance'], []), root: 'modules/other' },
];
const areaCtx = {
  owners: parseOwners('shared = ["dkrt::entry"]\n"C:boot" = ["dkrt::boot"]\n"C:storage" = ["dkrt::storage"]'),
  serviceTags: new Set<string>(),
  recordTags: new Set(['S:lfo']),
  portUsers: (file: string) => (file === 'ports/fw/lfo.toml' ? ['core/services/lfo'] : []),
  deviceOf: (file: string) => (file === 'devices/x-device.toml' ? d3 : undefined),
  crateOf: (file: string) =>
    file.startsWith('runtime/core/src/rs/') ? { name: 'dkrt', dir: 'runtime/core/src/rs' } : undefined,
};
// A stand-in for Nx's answer (device-reach.mjs): these paths are inputs of device tests, the others are not.
const fakeReaches = (files: string[]) =>
  files.filter(
    (f) =>
      /^(packages\/patchkit\/|sdk\/|runtime\/core\/owners|h\/)/.test(f) ||
      /^runtime\/core\/src\/rs\/src\/(newmod|entry)\.rs$/.test(f),
  );
const selected = (changed: string[], tags: string[] = [], closureOf?: (r: string, f: string) => string[]) => {
  const got = areaSelection(tags, areaGraph, { changed, ctx: areaCtx, closureOf, reaches: fakeReaches });
  return {
    got,
    pairs: selectionPairs(got.tests, areaGraph, [DEVICE_SMOKE]).map((p) => `${p.project}:${p.target}`),
  };
};

it('selects the tests of the area of a runtime-only change, not of every record that depends on the runtime', () => {
  const { got, pairs } = selected(['runtime/core/src/rs/src/boot.rs']);
  expect(got.mapped).toEqual([{ file: 'runtime/core/src/rs/src/boot.rs', areas: ['C:boot'] }]);
  expect(pairs).toEqual([
    `core/rt-tests:test-device--boot--${d1}`,
    `core/rt-tests:test-device--boot--${d2}`,
    'gate-device-smoke:device',
  ]);
});

it('goes wide for a shared runtime module, and names the file and why', () => {
  const { got } = selected(['runtime/core/src/rs/src/entry.rs']);
  expect(got.wideAll).toBe(true);
  expect(got.wideDevices).toBeNull();
  expect(got.wide[0]).toMatchObject({ file: 'runtime/core/src/rs/src/entry.rs' });
  expect(got.wide[0]!.why).toContain('shared runtime module dkrt::entry');
});

it('goes wide for a path with no area that reaches device tests, whatever else changed', () => {
  for (const file of ['packages/patchkit/src/x.rs', 'runtime/core/owners.toml', 'sdk/sdk.toml']) {
    const { got } = selected(['runtime/core/src/rs/src/boot.rs', file]);
    expect(got.wideAll, file).toBe(true);
    expect(got.wide.map((w: { file: string }) => w.file)).toEqual([file]);
  }
  // an unowned file of the runtime is wide, not smoke only
  expect(selected(['runtime/core/src/rs/src/newmod.rs']).got.wideAll).toBe(true);
});

it('stays precise when every path that reaches device tests names an area; a path no test reaches is ignored', () => {
  const { got } = selected(['runtime/core/src/rs/src/boot.rs', 'docs/x.md', 'apps/web/a.ts']);
  expect(got.wideAll).toBe(false);
  expect(got.wide).toEqual([]);
});

it('maps the port file of one plug-in point to its area, not to wide', () => {
  const { got } = selected(['ports/fw/lfo.toml']);
  expect(got.wideAll).toBe(false);
  expect(got.mapped).toEqual([{ file: 'ports/fw/lfo.toml', areas: ['S:lfo'] }]);
});

it('widens a device file to the halves of its device only', () => {
  const { got } = selected(['devices/x-device.toml']);
  expect(got.wideAll).toBe(true);
  expect(got.wideDevices).toEqual([d3]);
  const halves = [`a:test-device--x--${d1}`, `a:test-device--x--${d3}`, `b:test-device--${d3}`];
  expect(halvesOfDevices(halves, got.wideDevices)).toEqual([`a:test-device--x--${d3}`, `b:test-device--${d3}`]);
  // with a path that has no area the device filter is gone
  expect(selected(['devices/x-device.toml', 'sdk/sdk.toml']).got.wideDevices).toBeNull();
  expect(halvesOfDevices(halves, null)).toEqual(halves);
});

it('selects the conformance of a changed module, all of it for loading, types or a wide path', () => {
  const affected = new Set(['core/m:test-conformance', 'core/other:test-conformance', 'core/m:pack']);
  const conf = (changed: string[]) => {
    const got = selected(changed).got;
    return conformanceSelection(affected, areaGraph, { changed, tags: got.tags, wideAll: got.wideAll });
  };
  expect(conf(['modules/m/src/a.c'])).toEqual(['core/m:test-conformance']);
  expect(conf(['runtime/core/src/rs/src/boot.rs'])).toEqual([]);
  expect(conf(['packages/patchkit/src/x.rs'])).toEqual(['core/m:test-conformance', 'core/other:test-conformance']);
  const loading = conformanceSelection(affected, areaGraph, { changed: [], tags: ['C:loading'], wideAll: false });
  expect(loading).toHaveLength(2);
});

it('selects the records of a changed module and the smoke set', () => {
  const { got, pairs } = selected(['modules/m/src/a.c']);
  expect(got.mapped).toEqual([]);
  expect(pairs).toEqual([`core/m:test-device--${d1}`, 'gate-device-smoke:device']);
});

it('selects the tests that import a changed harness file, and no others', () => {
  const closureOf = (root: string, file: string) => [`${root}/${file}`, ...(root === 'modules/m' ? ['h/kit.ts'] : [])];
  expect(selected(['h/kit.ts'], [], closureOf).pairs).toEqual([
    `core/m:test-device--${d1}`,
    'gate-device-smoke:device',
  ]);
});

it('adds the tests of the given tags to the tests of the changed paths', () => {
  const { pairs } = selected(['runtime/core/src/rs/src/boot.rs'], ['C:storage']);
  expect(pairs).toContain(`core/rt-tests:test-device--storage--${d2}`);
  expect(pairs).toContain(`core/rt-tests:test-device--boot--${d1}`);
  expect(selected([], ['C:state']).pairs).toEqual([`core/m:test-device--${d1}`, 'gate-device-smoke:device']);
  expect(selected([], ['S:fx']).got.unknown).toEqual(['S:fx']);
  // a tag no record carries that a path derives is no error
  expect(
    areaSelection([], [areaGraph[3]!], {
      changed: ['runtime/core/src/rs/src/boot.rs'],
      ctx: areaCtx,
      reaches: fakeReaches,
    }).unknown,
  ).toEqual([]);
});

it('runs the smoke set in one call with the area tests', () => {
  const { pairs } = selected(['runtime/core/src/rs/src/boot.rs']);
  const calls = invocations(
    pairs.map((p) => ({ project: p.split(':')[0]!, target: p.split(':')[1]! })),
    areaGraph,
  );
  expect(calls.flat().join(' ')).toContain('gate-device-smoke');
});

it('reports the device halves that affected selects and the area gate leaves out', () => {
  const affected = new Set([
    'core/rt-tests:test-device',
    `core/rt-tests:test-device--boot--${d1}`,
    `core/rt-tests:test-device--storage--${d1}`,
    `core/m:test-device--${d1}`,
    'core/m:pack',
  ]);
  expect(deviceHalves(affected)).toEqual([
    `core/rt-tests:test-device--boot--${d1}`,
    `core/rt-tests:test-device--storage--${d1}`,
    `core/m:test-device--${d1}`,
  ]);
  const skipped = skippedHalves(
    affected,
    [{ project: 'core/rt-tests', target: `test-device--boot--${d1}` }],
    [`core/m:test-device--${d1}`],
  );
  expect(skipped).toEqual([`core/rt-tests:test-device--storage--${d1}`]);
  expect(skipText(skipped)).toBe('1 device halves and conformance tasks of 1 projects: core/rt-tests 1');
});

it('names gate-smoke and gate-pins as affected when only a dependency selects their task', () => {
  // `nx show projects --affected` misses these; the task graph of `nx affected` has them
  const tasks = new Set(['gate-smoke:smoke', 'digi-control:daemons']);
  expect(
    affectedFixed('B', (_b: string, target: string) => projectsWithTarget(tasks, target)).map((f) => f.project),
  ).toEqual(['gate-smoke']);
});

it('lists the smoke halves from the dependencies of the smoke target', () => {
  const smoke = {
    name: 'gate-device-smoke',
    targetDeps: { device: [{ projects: ['core/x'], target: `test-device--a--${d1}` }] },
  };
  expect(smokeHalves([smoke])).toEqual([`core/x:test-device--a--${d1}`]);
});

it('refuses only a base with no changed file', () => {
  expect(baseProblem({ base: 'B', changed: [] })).toMatch(/no file changed/);
  expect(baseProblem({ base: 'B', changed: ['docs/x.md'] })).toBe('');
  expect(baseProblem({ changed: [] })).toBe('');
});

it('makes the merge tier select device tests by area unless all affected is asked for', () => {
  expect(deviceByArea(TIERS.merge, false)).toBe(true);
  expect(deviceByArea(TIERS.merge, true)).toBe(false);
  expect(deviceByArea(TIERS.full, false)).toBe(false);
  expect(deviceByArea(TIERS.changed, false)).toBe(false);
});

it('refuses to select without a reach function: no default narrows silently', () => {
  expect(() => areaSelection([], areaGraph, { changed: ['x'], ctx: areaCtx } as never)).toThrow(/reaches/);
});

it('selects the record a path is listed in `sources` of, and does not go wide for it', () => {
  const graph = [
    {
      ...project(
        'core/s',
        [`test-device--${d1}`],
        [{ files: ['tests/s.device.test.ts'], tags: ['C:state'], sources: ['tools/s/input.txt'] }],
      ),
      root: 'modules/s',
    },
  ];
  const got = areaSelection([], graph, {
    changed: ['tools/s/input.txt'],
    ctx: areaCtx,
    reaches: () => ['tools/s/input.txt'],
  });
  expect(got.wideAll).toBe(false);
  expect(got.tests.map((t: { project: string }) => t.project)).toEqual(['core/s']);
});

it('asks reaches only about paths no area and no record covers', () => {
  const asked: string[][] = [];
  areaSelection([], areaGraph, {
    changed: ['runtime/core/src/rs/src/boot.rs', 'modules/m/src/a.c', 'docs/x.md'],
    ctx: areaCtx,
    reaches: (f: string[]) => (asked.push(f), []),
  });
  expect(asked).toEqual([['docs/x.md']]);
});

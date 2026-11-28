// `mise run test:subset -- [<project...>] [--base REF] [--why] [--run] [--gate]`: the device tests of the given projects as Nx leaf
// targets, plus gate-smoke and gate-pins when Nx `affected` selects them (against --base, else DIGI_GATE_BASE, else main, as gate.mjs;
// if the affected call fails it stops with one line naming the base). Without --run it prints them and how many start at once by DSP
// mode; with --run it runs them through `pnpm nx:pilot run-many` (DIGI_EMU_SLOTS and the other emulator settings pass through the environment).
// A named project selects its own device tests and the tests of every project that names it in `implicitDependencies`.
// `--why` adds a line to each selected file: the names that selected it, and the changed files that selected it with `--base`.
// `--base REF` (and `--gate`) adds the tests the changed files select by ownership (tools/nx/device-owners.mjs, docs/TESTING.md
// "Device test selection"): the owned tests of the touched projects, the tests that name them, the tests that load a changed fixture
// and the tests that import a changed file, plus the fixed smoke set gate-device-smoke when `affected` selects a device test. Runtime
// source files are decided by area first (tools/nx/runtime-areas.json, docs/TESTING.md "Runtime areas"): the mapped tests of the area, or none for a host-proven
// file (a file in neither fails the gate). A changed path no project owns, or that no test names, selects nothing and is printed. What `affected` selects and the gate skips is listed.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { budget, startAtOnce, workCores } from './resources.mjs';
import { closureFiles, testImportInputs } from './ts-imports.mjs';
import { gateContext } from './gate-context.mjs';
import { areaSelection, portAreaSelection } from './runtime-areas.mjs';
import { applyConfinement, confinedToSelection } from './selection-files.mjs';
import { DEVICES, leavesOf, ownedConformance, ownershipSelection } from './device-owners.mjs';

export { DEVICES };

// Gate projects whose suites join a subset when `affected` selects them: the patchkit smoke matrix and the PCM pins.
export const FIXED = [
  { project: 'gate-smoke', target: 'smoke' },
  { project: 'gate-pins', target: 'pins' },
];

// The fixed cross-service smoke set of the gate (tools/gate/device-smoke, docs/TESTING.md "Device smoke set").
export const DEVICE_SMOKE = { project: 'gate-device-smoke', target: 'device' };

// `projects`: [{ name, root, subjects, targets: string[], metadata }] (the nodes of the Nx project graph).
// The device tests of the named projects: a project's own records and the records of every project that names it in
// `implicitDependencies`. Returns { tests: [{ project, file, named: [], changed, dsp, targets }], unknown } where `targets` are the leaf
// targets of the file, one per device (leavesOf). `unknown` lists the names that are no project or select no device test.
export const namedSelection = (names, projects) => {
  const wanted = new Set(names);
  const found = new Set();
  const tests = [];
  for (const { name, subjects = [], targets, metadata } of projects) {
    const via = [name, ...subjects].filter((n) => wanted.has(n));
    if (!via.length) continue;
    for (const rec of metadata?.runtimeAcceptance ?? [])
      for (const file of rec.files ?? []) {
        const leaves = leavesOf(file, targets);
        if (!leaves.length) continue;
        via.forEach((n) => found.add(n));
        tests.push({ project: name, file, named: via, changed: [], dsp: rec.dsp ?? 'zero', targets: leaves });
      }
  }
  return { tests, unknown: names.filter((n) => !found.has(n)) };
};

// The `--why` line of a selected test: the given names it has, and the changed files that selected it (`--base`).
export const whyText = (t) =>
  [
    ...t.named,
    ...t.changed.slice(0, 3).map((f) => `changed ${f}`),
    ...(t.changed.length > 3 ? [`${t.changed.length - 3} more changed`] : []),
  ].join(', ');

// The tasks of a selection by DSP mode, and how many start at once with the machine's core budget and slots.
// Whether the fixed device smoke set joins: `affected` selects a device half, unless the change is confined to the gate's own selection
// files (selection-files.mjs), which no device run proves.
export const smokeSelected = (changed, affected) => !confinedToSelection(changed) && deviceHalves(affected).length > 0;

export const modeSummary = (tests, work = workCores()) => {
  const count = { sharc: 0, zero: 0 };
  for (const t of tests) count[t.dsp] += t.targets.length;
  const b = budget(work, process.env.DIGI_BUDGET);
  return { count, cores: b.budget, slots: b.tasks, once: startAtOnce(count, b.budget, b.tasks) };
};

// The `run-many` calls for the selection. One call when the cross product of its projects and targets is exact (a project that does not
// have a target is skipped by Nx); else one call per target.
export const invocations = (pairs, projects) => {
  const have = new Map(projects.map((p) => [p.name, new Set(p.targets)]));
  const wanted = new Set(pairs.map((p) => `${p.project}:${p.target}`));
  const names = [...new Set(pairs.map((p) => p.project))];
  const targets = [...new Set(pairs.map((p) => p.target))];
  const exact = names.every((n) => targets.every((t) => !have.get(n)?.has(t) || wanted.has(`${n}:${t}`)));
  const call = (ts, ns) => ['run-many', '-t', ts.join(','), '-p', ns.join(',')];
  if (exact) return [call(targets, names)];
  return targets.map((t) => call([t], [...new Set(pairs.filter((p) => p.target === t).map((p) => p.project))]));
};

// `fixed`: the FIXED entries that Nx `affected` selects (see affectedFixed); the others are left out.
export const selectionPairs = (tests, projects, fixed = FIXED) => {
  const known = new Set(projects.map((p) => p.name));
  const added = fixed.filter((f) => known.has(f.project));
  return [...tests.flatMap((t) => t.targets.map((target) => ({ project: t.project, target }))), ...added];
};

// The FIXED entries whose project Nx `affected` selects against `base`. `affected(base, target)`: the affected project names.
export const affectedFixed = (base, affected = defaultAffected) =>
  FIXED.filter((f) => affected(base, f.target).includes(f.project));
const defaultAffected = (base, target) => projectsWithTarget(affectedTasks(base), target);
// The projects among `tasks` (ids `project:target`) that run `target`.
export const projectsWithTarget = (tasks, target) =>
  [...tasks].filter((id) => id.endsWith(`:${target}`)).map((id) => id.slice(0, id.lastIndexOf(':')));

// The tasks `nx affected` selects for the device, smoke and pin targets against `base`: the question `mise run test:plan` asks, at the task
// level (a task is affected when an input or a dependency's output changed), not the project level of `nx show projects --affected`,
// which misses gate-smoke and gate-pins when only a dependency such as the daemons changed. Cached per base.
const affectedCache = new Map();
export const affectedTasks = (base) => {
  if (affectedCache.has(base)) return affectedCache.get(base);
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const dir = resolve(root, 'out/runs/test-subset');
  mkdirSync(dir, { recursive: true });
  const file = resolve(dir, 'affected.json');
  rmSync(file, { force: true });
  const done = spawnSync(
    'node',
    [
      'tools/nx/nx.mjs',
      'affected',
      `--base=${base}`,
      '-t',
      'test-device,test-conformance,smoke,pins',
      `--graph=${file}`,
    ],
    { cwd: root, encoding: 'utf8' },
  );
  if (done.status !== 0) {
    console.error(
      `test:subset: nx affected failed against base ${base}; set the base with --base REF or DIGI_GATE_BASE`,
    );
    process.exit(2);
  }
  let tasks = [];
  try {
    tasks = Object.keys(JSON.parse(readFileSync(file, 'utf8')).tasks?.tasks ?? {});
  } catch {
    /* Nx writes no graph when nothing is affected. */
  }
  affectedCache.set(base, new Set(tasks));
  return affectedCache.get(base);
};

// The device-half tasks (`project:test-device--<file>--<dev>`) among `tasks`: what the old gate ran.
export const deviceHalves = (tasks) =>
  [...tasks].filter((id) => new RegExp(`^[^:]+:test-device--(?:.+--)?(?:${DEVICES.join('|')})$`).test(id));

// The conformance tasks (`project:test-conformance`) among `tasks`.
export const conformanceTasks = (tasks) => [...tasks].filter((id) => id.endsWith(':test-conformance'));

// The halves of the smoke set, from the dependencies of its target in the graph nodes (`projects[].targetDeps`).
export const smokeHalves = (projects) =>
  (projects.find((p) => p.name === DEVICE_SMOKE.project)?.targetDeps?.[DEVICE_SMOKE.target] ?? []).flatMap((d) =>
    (d.projects ?? []).map((project) => `${project}:${d.target}`),
  );

// `chosen`: the `{ project, target }` pairs of the selection (the smoke set included). Returns what `affected` selects and the gate skips.
export const skippedHalves = (affected, chosen, smoke = []) => {
  const run = new Set([...chosen.map((p) => `${p.project}:${p.target}`), ...smoke]);
  return [...deviceHalves(affected), ...conformanceTasks(affected)].filter((id) => !run.has(id));
};

// `skipped`: task ids -> the line the gate prints, by project.
export const skipText = (skipped) => {
  const by = new Map();
  for (const id of skipped) by.set(id.split(':')[0], (by.get(id.split(':')[0]) ?? 0) + 1);
  const list = [...by].map(([project, n]) => `${project} ${n}`);
  return `${skipped.length} device halves and conformance tasks of ${by.size} projects: ${list.slice(0, 12).join(', ')}${list.length > 12 ? `, ${list.length - 12} more projects` : ''}`;
};

// The halves among `halves` (`project:test-device--...--<dev>`) of the devices in `devices` (all when null).
export const halvesOfDevices = (halves, devices) =>
  devices ? halves.filter((id) => devices.some((d) => id.endsWith(`--${d}`))) : halves;

// Files changed since `base`: the tracked diff (committed and working tree) and the untracked files. `git`: runs git in `root`.
export const changedFiles = (root, base, git = defaultGit) => [
  ...new Set([...git(root, 'diff', '--name-only', base), ...git(root, 'ls-files', '--others', '--exclude-standard')]),
];
const defaultGit = (root, ...args) => {
  const done = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
  if (done.status !== 0) throw new Error(`git ${args.join(' ')} failed:\n${done.stderr}`);
  return done.stdout.split('\n').filter(Boolean);
};

// Why a `--base` selection must not run, or '': nothing changed since the base. A change that selects no device test is no problem
// (a host-only change runs none); the smoke set and the skipped list say what the gate did. For the merge gate (`gate`) an empty
// change is no problem either: there is nothing to run.
export const baseProblem = ({ base, changed, gate = false }) =>
  base && !changed.length && !gate ? `no file changed since ${base}.` : '';

// A file at `ref` (null when it does not exist there) and in the working tree (null when deleted).
const gitShow = (root, ref, file) => {
  const done = spawnSync('git', ['show', `${ref}:${file}`], { cwd: root, encoding: 'utf8' });
  return done.status === 0 ? done.stdout : null;
};
const readIfExists = (file) => (existsSync(file) ? readFileSync(file, 'utf8') : null);

export const loadProjects = (root) => {
  const dir = resolve(root, 'out/runs/test-subset');
  mkdirSync(dir, { recursive: true });
  const file = resolve(dir, 'graph.json');
  const done = spawnSync('node', ['tools/nx/nx.mjs', 'graph', `--file=${file}`], { cwd: root, encoding: 'utf8' });
  if (done.status !== 0) throw new Error(`nx graph failed:\n${done.stderr || done.stdout}`);
  const nodes = JSON.parse(readFileSync(file, 'utf8')).graph.nodes;
  return Object.values(nodes).map((n) => ({
    name: n.name,
    root: n.data.root,
    subjects: n.data.implicitDependencies ?? [],
    targets: Object.keys(n.data.targets ?? {}),
    targetDeps: Object.fromEntries(Object.entries(n.data.targets ?? {}).map(([t, v]) => [t, v.dependsOn ?? []])),
    metadata: n.data.metadata,
  }));
};

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const run = args.includes('--run');
  const why = args.includes('--why');
  const gate = args.includes('--gate');
  const at = args.indexOf('--base');
  const base = at >= 0 ? args[at + 1] : gate ? (process.env.DIGI_GATE_BASE ?? 'main') : undefined;
  const flags = ['--run', '--why', '--gate', '--', '--base'];
  const names = args.filter((a, i) => !flags.includes(a) && !(at >= 0 && i === at + 1));
  if ((!names.length && !base) || (at >= 0 && !base) || names.some((t) => t.startsWith('-'))) {
    console.error('usage: mise run test:subset -- [<project...>] [--base REF] [--why] [--run] [--gate]');
    process.exit(2);
  }
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const projects = loadProjects(root);
  let selection;
  if (base) {
    // Ownership gate (device-owners.mjs): what owns a changed file selects the tests; the named projects join as a union.
    const changed = changedFiles(root, base);
    const importInputs = testImportInputs(root);
    const ctx = gateContext(root, base);
    // Runtime source files are decided by their area (runtime-areas.json); the rest by ownership.
    const area = areaSelection(projects, changed);
    if (area.unclassified.length) {
      console.error(
        `test:subset: runtime file(s) in no area and not host-proven: ${area.unclassified.slice(0, 10).join(' ')}; add them to tools/nx/runtime-areas.json`,
      );
      process.exit(2);
    }
    const owned = ownershipSelection(projects, {
      changed: changed.filter((f) => !area.governed.has(f)),
      closureOf: (projectRoot, file) => closureFiles(importInputs, `${projectRoot}/${file}`),
      portUsers: ctx.portUsers,
      portEntries: ctx.portEntries,
      deviceOf: ctx.deviceOf,
      projectJsonOf: (file) => ({ before: gitShow(root, base, file), after: readIfExists(resolve(root, file)) }),
    });
    // A changed port binding also selects the area of the platform module that declares it.
    const declaring = portAreaSelection(projects, owned.mapped);
    const named = namedSelection(names, projects);
    // A change confined to the gate's own selection files selects no area or ownership test; named projects still join.
    selection = applyConfinement(
      {
        ...owned,
        tests: [...area.tests, ...declaring.tests, ...owned.tests],
        unknown: named.unknown,
        areaNotes: [...area.notes, ...declaring.notes],
        deviceFiles: owned.devices,
        changed,
      },
      named.tests,
      changed,
    );
  } else
    selection = {
      ...namedSelection(names, projects),
      mapped: [],
      deviceFiles: [],
      changed: [],
    };
  const { tests, unknown, mapped, deviceFiles } = selection;
  const problem = baseProblem({ base, changed: selection.changed, gate });
  if (gate && base && !selection.changed.length) {
    console.log(`test:subset: no file changed since ${base}: no device tests to run.`);
    process.exit(0);
  }
  if (problem) {
    console.error(`test:subset: ${problem}`);
    process.exit(2);
  }
  if (unknown.length) {
    console.error(
      `test:subset: no device test belongs to the project ${unknown.join(' ')} (a name is an Nx project with device tests, or one that a test project names in implicitDependencies)`,
    );
    process.exit(2);
  }
  // The merge gate (`--gate`) runs gate-smoke and gate-pins itself with the other targets; a named subset adds them when affected selects them.
  const affected = base ? affectedTasks(base) : new Set();
  const fixed = gate ? [] : affectedFixed(base ?? process.env.DIGI_GATE_BASE ?? 'main');
  // The device smoke set runs whenever `affected` selects a device half.
  const smoke = base && smokeSelected(selection.changed, affected) ? [DEVICE_SMOKE] : [];
  // A changed device file runs every device half `affected` selects of that device; the conformance tasks are those of
  // ownedConformance.
  const deviceHalvesOf = deviceFiles.length ? halvesOfDevices(deviceHalves(affected), deviceFiles) : [];
  const conformance = base ? ownedConformance(affected, projects, selection.changed) : [];
  const idPair = (id) => ({ project: id.slice(0, id.indexOf(':')), target: id.slice(id.indexOf(':') + 1) });
  const pairs = [
    ...new Map(
      [
        ...selectionPairs(tests, projects, [...fixed, ...smoke]),
        ...[...deviceHalvesOf, ...conformance].map(idPair),
      ].map((p) => [`${p.project}:${p.target}`, p]),
    ).values(),
  ];
  console.log(`test:subset: ${[...names, ...(base ? [`--base ${base}`] : [])].join(' ')}`);
  for (const note of selection.areaNotes ?? []) console.log(`  ${note}`);
  for (const { file, project } of mapped.slice(0, 20)) console.log(`  owner ${project} <- ${file}`);
  if (mapped.length > 20) console.log(`  owner: ${mapped.length - 20} more changed files`);
  for (const { file, why } of (selection.unowned ?? []).slice(0, 20))
    console.log(`  unowned ${file} (${why}): selects no test`);
  for (const { file, project } of (selection.metadataOnly ?? []).slice(0, 20))
    console.log(`  metadata only ${project}: ${file} selects none (tags, subjects or reasons changed)`);
  for (const { file, project } of (selection.quiet ?? []).slice(0, 20))
    console.log(`  no test names ${project}: ${file} selects none (smoke and the full tier cover it)`);
  if (deviceFiles.length) console.log(`  device files: every affected half of ${deviceFiles.join(' ')}`);
  if (base)
    console.log(
      `  conformance: ${conformance.length ? conformance.map((id) => id.split(':')[0]).join(' ') : 'none selected'}`,
    );
  for (const t of tests) {
    console.log(`  ${t.project} ${t.file}: ${t.targets.join(' ')}`);
    if (why) console.log(`    why: ${whyText(t)}`);
  }
  if (!gate)
    for (const f of FIXED)
      console.log(fixed.includes(f) ? `  ${f.project}: ${f.target}` : `  ${f.project}: not affected`);
  if (base)
    console.log(
      smoke.length
        ? `  ${DEVICE_SMOKE.project}: ${DEVICE_SMOKE.target} (${smokeHalves(projects).length} halves)`
        : `  ${DEVICE_SMOKE.project}: not selected (${confinedToSelection(selection.changed) ? 'only gate selection files changed' : 'no device input changed'})`,
    );
  if (base) {
    const skipped = skippedHalves(affected, pairs, smokeHalves(projects));
    console.log(
      skipped.length
        ? `test:subset: skipped, selected by affected and left to the full tier (mise run test -- --all-affected or test:full): ${skipText(skipped)}`
        : 'test:subset: skipped: no affected device half left out',
    );
  }
  const m = modeSummary(tests);
  console.log(
    `test:subset: ${m.count.sharc} coupled and ${m.count.zero} zero-DSP tasks; core budget ${m.cores}, ${m.slots} slots: ` +
      `${m.once.sharc} coupled and ${m.once.zero} zero-DSP start at once`,
  );
  if (!pairs.length) {
    console.log('test:subset: nothing to run');
    process.exit(0);
  }
  const calls = invocations(pairs, projects);
  for (const c of calls) console.log(`test:subset: pnpm nx:pilot ${c.join(' ')}`);
  if (!run) process.exit(0);
  for (const c of calls) {
    const done = spawnSync(
      'node',
      [
        'tools/nx/remote-cache.mjs',
        'exec',
        '--access',
        'rw',
        '--',
        'pnpm',
        'nx:pilot',
        ...c,
        '--output-style=static-failures-only',
      ],
      { cwd: root, stdio: 'inherit', env: process.env },
    );
    if (done.status !== 0) process.exit(done.status ?? 1);
  }
}

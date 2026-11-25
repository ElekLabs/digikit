// `mise run test:subset -- [<tags...>] [--base REF] [--why] [--run]`: the device tests of the given record tags (tools/nx/acceptance-tags.mjs)
// as Nx leaf targets, plus gate-smoke and gate-pins when Nx `affected` selects them (against --base, else DIGI_GATE_BASE, else main, as gate.mjs; if the affected call fails it stops with one line naming the base). Without --run it prints them and how many start at once by DSP mode;
// with --run it runs them through `pnpm nx:pilot run-many` (DIGI_EMU_SLOTS and the other emulator settings pass through the environment).
// `--why` adds a line to each selected file: the given tags it has, and the changed files that selected it with `--base`.
// Tags of one call are a union: a test runs when any of its record tags is given.
// `--base REF` adds the tests whose subject changed since REF (`git diff REF`, with the working tree): a record's subject is the
// source of its project (not its tests) and of the projects of its `modules`, so a change to a machine selects the records that load it.
// Ownership gate (`--base`, and `--gate`): tools/nx/device-owners.mjs selects by the project that owns a changed file; the paragraph below describes the old area layer (area-paths.mjs, unused by the gate, removed by U5).
// Area gate (old): the changed paths also name areas (tools/nx/area-paths.mjs: owners.toml and the
// service directories, firmware declaration files by their users, device files), and the tests of those areas join the selection with the
// fixed smoke set gate-device-smoke. Precise when the area is known, wide when it is not: when any changed path that reaches device tests
// names no area and is an input of any device or conformance test task as Nx resolves it (device-reach.mjs: a shared module, the SDK, patchkit, owners.toml, config, device data, bindings, image-module sources), every device test `affected` selects runs, and the gate prints the
// path that forced it; a device file alone widens to the halves of its device. Conformance follows the same rule (a module's own
// conformance on its changes; the loading and types areas, or a wide path: all). What `affected` selects and the gate skips is listed.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AREAS, SERVICES } from './acceptance-tags.mjs';
import { isTestSource } from './test-source.mjs';
import { SHARED_PROJECTS } from './device-tests.mjs';
import { budget, startAtOnce, workCores } from './resources.mjs';
import { closureFiles, testImportInputs } from './ts-imports.mjs';
import { areaContext, areasOfPath } from './area-paths.mjs';
import { ownedConformance, ownershipSelection } from './device-owners.mjs';

// Gate projects whose suites join a subset when `affected` selects them: the patchkit smoke matrix and the PCM pins.
export const FIXED = [
  { project: 'gate-smoke', target: 'smoke' },
  { project: 'gate-pins', target: 'pins' },
];

// The fixed cross-service smoke set of the area gate (tools/gate/device-smoke, docs/TESTING.md "Device smoke set").
export const DEVICE_SMOKE = { project: 'gate-device-smoke', target: 'device' };

const stemOf = (file) =>
  file
    .split('/')
    .at(-1)
    .replace(/\.device\.test\.ts$/, '');
export const DEVICES = ['dt2', 'dn2', 'st'];

// Whether a changed file is production source of the project at `root`: below it, and not a device test (the project tests/conformance
// keeps them at its root, outside a tests directory), a test source (tools/nx/test-source.mjs), a Markdown file or its project.json.
const isSource = (file, root) =>
  file.startsWith(`${root}/`) &&
  !file.endsWith('.device.test.ts') &&
  !file.endsWith('.md') &&
  !file.endsWith('/project.json') &&
  !isTestSource(file.slice(root.length + 1));

// The roots of the projects a record's subject is made of: its own project, the projects its test-only project tests, and the
// modules it loads. A module id that is no project is an error: a silent miss would drop the record from the selection.
const subjectRoots = (rec, root, subjects, rootOf, skip = new Set()) =>
  [root, ...subjects, ...(rec.modules ?? [])]
    .filter((id) => id === root || !skip.has(id))
    .map((id) => {
      if (id === root) return root;
      const found = rootOf.get(id);
      if (found === undefined)
        throw new Error(`test:subset: ${id} (of a runtimeAcceptance record in ${root}) is no Nx project`);
      return found;
    });

// The changed files that are source of a record's subject, or a test file it lists.
export const subjectFiles = (rec, root, changed, rootOf, subjects = [], skip = new Set()) => {
  const roots = subjectRoots(rec, root, subjects, rootOf, skip);
  const files = [...(rec.files ?? []).map((f) => `${root}/${f}`), ...(rec.sources ?? [])];
  return changed.filter((file) => files.includes(file) || roots.some((r) => isSource(file, r)));
};

// `projects`: [{ name, root, subjects, targets: string[], metadata }] (the nodes of the Nx project graph).
// `opts.changed`: changed files relative to the repository root: records whose subject changed, or whose test files import a changed
// file (`opts.closureOf(projectRoot, file)`: the import closure of a test file, repository-relative), join the tag selection.
// Returns { tests: [{ project, file, tags, changed, dsp, targets }], unknown, covered } where `targets` are the leaf targets of
// the file, one per device it runs on: `test-device--<file>--<dev>`, or `test-device--<dev>` for a project with one device test file.
// `covered` is the set of changed files a record's subject, sources or test imports cover.
// `opts.excludeSubjects`: project names that are no subject of a record (the area gate maps the paths of those projects by area:
// tools/nx/area-paths.mjs), so a change below `core/runtime` selects no record through a test project's dependency on it.
export const resolveSubset = (tags, projects, opts = {}) => {
  const { changed = [], closureOf } = opts;
  const skip = new Set(opts.excludeSubjects ?? []);
  const wanted = new Set(tags);
  const found = new Set();
  const tests = [];
  const covered = new Set();
  const rootOf = new Map(projects.map((p) => [p.name, p.root]));
  const tsChanged = changed.filter((f) => f.endsWith('.ts'));
  for (const { name, root, subjects = [], targets, metadata } of projects) {
    const have = new Set(targets);
    for (const rec of metadata?.runtimeAcceptance ?? []) {
      const own = (rec.tags ?? []).filter((tag) => wanted.has(tag));
      let byPath = false;
      let viaFiles = [];
      if (changed.length && root !== undefined) {
        const hits = subjectFiles(rec, root, changed, rootOf, subjects, skip);
        hits.forEach((f) => covered.add(f));
        byPath = hits.length > 0;
        viaFiles = hits;
        if (!byPath && closureOf && tsChanged.length) {
          const reached = new Set();
          for (const f of rec.files ?? []) {
            const closure = new Set(closureOf(root, f));
            tsChanged.filter((c) => closure.has(c)).forEach((c) => reached.add(c));
          }
          viaFiles = [...reached];
          viaFiles.forEach((f) => covered.add(f));
          byPath = viaFiles.length > 0;
        }
      }
      if (!own.length && !byPath) continue;
      own.forEach((tag) => found.add(tag));
      for (const file of rec.files ?? []) {
        const stem = stemOf(file);
        const leaves = DEVICES.flatMap((dev) =>
          [`test-device--${stem}--${dev}`, `test-device--${dev}`].filter((t) => have.has(t)).slice(0, 1),
        );
        if (leaves.length)
          tests.push({ project: name, file, tags: own, changed: viaFiles, dsp: rec.dsp ?? 'zero', targets: leaves });
      }
    }
  }
  return { tests, unknown: tags.filter((tag) => !found.has(tag)), covered };
};

// The `--why` line of a selected test: the given tags it has, and the changed files that selected it (`--base`).
export const whyText = (t) =>
  [
    ...t.tags,
    ...t.changed.slice(0, 3).map((f) => `changed ${f}`),
    ...(t.changed.length > 3 ? [`${t.changed.length - 3} more changed`] : []),
  ].join(', ');

// The tasks of a selection by DSP mode, and how many start at once with the machine's core budget and slots.
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

// The areas whose change reaches every conformance test: the runtime paths of loading modules and of the machine types.
export const CONFORMANCE_AREAS = ['C:loading', 'C:types'];

// The halves among `halves` (`project:test-device--...--<dev>`) of the devices in `devices` (all when null).
export const halvesOfDevices = (halves, devices) =>
  devices ? halves.filter((id) => devices.some((d) => id.endsWith(`--${d}`))) : halves;

// Tags and changed paths -> the selection of the area gate: the tests of the given tags, of the areas the paths name, of the records
// whose own project or modules changed and of the tests that import a changed file. `ctx`: area-paths.mjs `areaContext`.
// Returns what `resolveSubset` returns, with `mapped` ([{ file, areas }]) and `wide`: [{ file, why, devices? }], the changed paths that
// reach device tests and name no area (`devices`: a device file, which widens to the halves of that device only), and `wideDevices`:
// null when the selection is not wide or a path widens it for every device, else the devices to widen to; `wideAll` says the selection
// is wide. `reaches(files)` (device-reach.mjs) returns the files that are inputs of a device or conformance task; a path no such task reads is ignored.
export const areaSelection = (tags, projects, { changed, closureOf, ctx, reaches }) => {
  if (!reaches)
    throw new Error('areaSelection: `reaches` is required (device-reach.mjs): no default may narrow silently');
  const mapped = [];
  const wide = [];
  const derived = new Set();
  const unnamed = [];
  for (const file of changed) {
    const found = areasOfPath(file, ctx);
    if (found.areas) {
      mapped.push({ file, areas: found.areas });
      found.areas.forEach((a) => derived.add(a));
    } else if (found.devices)
      wide.push({ file, why: `device file of ${found.devices.join(' ')}`, devices: found.devices });
    else if (found.unknown) unnamed.push({ file, why: found.unknown });
  }
  const all = [...new Set([...tags, ...derived])];
  const got = resolveSubset(all, projects, { changed, closureOf, excludeSubjects: SHARED_PROJECTS });
  // A path no area names is wide when it is an input of any device or conformance test task; a path a record's subject, sources or
  // test imports cover selects that record instead, and a path no such task reads is ignored.
  const open = unnamed.filter((u) => !got.covered.has(u.file));
  const reached = new Set(reaches(open.map((u) => u.file)));
  for (const u of open)
    if (reached.has(u.file)) wide.push({ file: u.file, why: `${u.why}; an input of the device or conformance tests` });
  const wideAll = wide.length > 0;
  const wideDevices = wideAll && wide.every((w) => w.devices) ? [...new Set(wide.flatMap((w) => w.devices))] : null;
  // A derived tag without a record is no error (the area has no device test); a tag the caller gave is.
  return {
    ...got,
    unknown: got.unknown.filter((t) => tags.includes(t)),
    mapped,
    wide,
    wideAll,
    wideDevices,
    tags: all,
  };
};

// The conformance tasks (`project:test-conformance`) of the area gate among the `affected` tasks: all when the selection is wide or an
// area of CONFORMANCE_AREAS changed, else the modules whose own source changed. `projects`: the graph nodes (name, root).
export const conformanceSelection = (affected, projects, { changed, tags, wideAll }) => {
  const have = conformanceTasks(affected);
  if (wideAll || tags.some((t) => CONFORMANCE_AREAS.includes(t))) return have;
  const roots = new Map(projects.map((p) => [p.name, p.root]));
  return have.filter((id) => {
    const root = roots.get(id.slice(0, -':test-conformance'.length));
    return root !== undefined && changed.some((file) => isSource(file, root));
  });
};

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

const vocabulary = () => [
  ...SERVICES.map((s) => `S:${s}`),
  ...AREAS.map((a) => `C:${a}`),
  'A:<core module id>',
  'M:<tool module id>',
];

const loadProjects = (root) => {
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
  const tags = args.filter((a, i) => !flags.includes(a) && !(at >= 0 && i === at + 1));
  if ((!tags.length && !base) || (at >= 0 && !base) || tags.some((t) => t.startsWith('-'))) {
    console.error(
      `usage: mise run test:subset -- [<tag...>] [--base REF] [--why] [--run] [--gate]\ntags: ${vocabulary().join(' ')}`,
    );
    process.exit(2);
  }
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const projects = loadProjects(root);
  let selection;
  if (base) {
    // Ownership gate (device-owners.mjs): what owns a changed file selects the tests; the given tags join as a union.
    const changed = changedFiles(root, base);
    const importInputs = testImportInputs(root);
    const ctx = areaContext(root, projects, base);
    const owned = ownershipSelection(projects, {
      changed,
      closureOf: (projectRoot, file) => closureFiles(importInputs, `${projectRoot}/${file}`),
      portUsers: ctx.portUsers,
      deviceOf: ctx.deviceOf,
    });
    const named = resolveSubset(tags, projects);
    const seen = new Set();
    selection = {
      ...owned,
      tests: [...owned.tests, ...named.tests].filter(
        (t) => !seen.has(`${t.project}:${t.file}`) && seen.add(`${t.project}:${t.file}`),
      ),
      unknown: named.unknown,
      wide: [],
      wideAll: owned.devices.length > 0,
      wideDevices: owned.devices.length ? owned.devices : null,
      tags,
      changed,
    };
  } else
    selection = {
      ...resolveSubset(tags, projects),
      mapped: [],
      wide: [],
      wideAll: false,
      wideDevices: null,
      tags,
      changed: [],
    };
  const { tests, unknown, mapped, wideAll, wideDevices } = selection;
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
      `test:subset: no device test has the tags ${unknown.join(' ')}\nvocabulary: ${vocabulary().join(' ')}`,
    );
    process.exit(2);
  }
  // The merge gate (`--gate`) runs gate-smoke and gate-pins itself with the other targets; a named subset adds them when affected selects them.
  const affected = base ? affectedTasks(base) : new Set();
  const fixed = gate ? [] : affectedFixed(base ?? process.env.DIGI_GATE_BASE ?? 'main');
  // The device smoke set runs whenever the old gate would have run a device test: some input of the device tests changed.
  const smoke = base && deviceHalves(affected).length ? [DEVICE_SMOKE] : [];
  // A wide selection runs every device half `affected` selects (of the devices a device file names, else all), and the conformance
  // tasks follow the rule of conformanceSelection.
  const wideHalves = wideAll ? halvesOfDevices(deviceHalves(affected), wideDevices) : [];
  const conformance = base ? ownedConformance(affected, projects, selection.changed) : [];
  const idPair = (id) => ({ project: id.slice(0, id.indexOf(':')), target: id.slice(id.indexOf(':') + 1) });
  const pairs = [
    ...new Map(
      [...selectionPairs(tests, projects, [...fixed, ...smoke]), ...[...wideHalves, ...conformance].map(idPair)].map(
        (p) => [`${p.project}:${p.target}`, p],
      ),
    ).values(),
  ];
  console.log(`test:subset: ${[...tags, ...(base ? [`--base ${base}`] : [])].join(' ')}`);
  for (const { file, project } of mapped.slice(0, 20)) console.log(`  owner ${project} <- ${file}`);
  if (mapped.length > 20) console.log(`  owner: ${mapped.length - 20} more changed files`);
  for (const { file, why } of (selection.unowned ?? []).slice(0, 20))
    console.log(`  unowned ${file} (${why}): selects no test`);
  for (const { file, project } of (selection.quiet ?? []).slice(0, 20))
    console.log(`  no test names ${project}: ${file} selects none (smoke and the full tier cover it)`);
  if (wideAll) console.log(`  device files: every affected half of ${wideDevices.join(' ')}`);
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
        : `  ${DEVICE_SMOKE.project}: not selected (no device input changed)`,
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

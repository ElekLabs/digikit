// `mise run test:subset -- [<tags...>] [--base REF] [--why] [--run]`: the device tests of the given record tags (tools/nx/acceptance-tags.mjs)
// as Nx leaf targets, plus gate-smoke and gate-pins. Without --run it prints them and how many start at once by DSP mode;
// with --run it runs them through `pnpm nx:pilot run-many` (DIGI_EMU_SLOTS and the other emulator settings pass through the environment).
// `--why` adds a line to each selected file: the given tags it has, and the changed files that selected it with `--base`.
// Tags of one call are a union: a test runs when any of its record tags is given.
// `--base REF` adds the tests whose subject changed since REF (`git diff REF`, with the working tree): a record's subject is the
// source of its project (not its tests) and of the projects of its `modules`, so a change to a machine selects the records that load it.
// Changes to shared inputs (SDK, profiles, patchkit) have no subject: name tags for them.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AREAS, SERVICES } from './acceptance-tags.mjs';
import { matchesGlob } from 'node:path';
import { SHARED_PROJECTS } from './device-tests.mjs';
import { NON_CELL_GLOBS, SHARED_INPUT_GLOBS } from './device-test-keys.mjs';
import { budget, startAtOnce, workCores } from './resources.mjs';
import { closureFiles, testImportInputs } from './ts-imports.mjs';

// Gate projects whose suites run with every subset: the patchkit smoke matrix and the PCM pins.
export const FIXED = [
  { project: 'gate-smoke', target: 'smoke' },
  { project: 'gate-pins', target: 'pins' },
];

const stemOf = (file) =>
  file
    .split('/')
    .at(-1)
    .replace(/\.device\.test\.ts$/, '');
const DEVICES = ['dt2', 'dn2', 'st'];

// Whether a changed file is production source of the project at `root`: below it, and not a test file, a Markdown file or its project.json.
const isSource = (file, root) =>
  file.startsWith(`${root}/`) &&
  !file.endsWith('.device.test.ts') &&
  !file.endsWith('.md') &&
  !file.endsWith('/project.json') &&
  !file.slice(root.length + 1).startsWith('tests/');

// The roots of the projects a record's subject is made of: its own project, the projects its test-only project tests, and the
// modules it loads. A module id that is no project is an error: a silent miss would drop the record from the selection.
const subjectRoots = (rec, root, subjects, rootOf) =>
  [root, ...subjects, ...(rec.modules ?? [])].map((id) => {
    if (id === root) return root;
    const found = rootOf.get(id);
    if (found === undefined)
      throw new Error(`test:subset: ${id} (of a runtimeAcceptance record in ${root}) is no Nx project`);
    return found;
  });

// The changed files that are source of a record's subject, or a test file it lists.
export const subjectFiles = (rec, root, changed, rootOf, subjects = []) => {
  const roots = subjectRoots(rec, root, subjects, rootOf);
  const files = (rec.files ?? []).map((f) => `${root}/${f}`);
  return changed.filter((file) => files.includes(file) || roots.some((r) => isSource(file, r)));
};

// Why a changed file is a shared input of every device test, or '' when it is not: the source of the projects every device task
// depends on (patchkit, the daemons, the runtime image), or a file the device-test keys hold for every test.
export const sharedInput = (file, projects) => {
  for (const name of SHARED_PROJECTS) {
    const root = projects.find((p) => p.name === name)?.root;
    if (root && isSource(file, root)) return `the source of ${name}`;
  }
  if (NON_CELL_GLOBS.some((g) => matchesGlob(file, g))) return '';
  const glob = SHARED_INPUT_GLOBS.find((g) => matchesGlob(file, g));
  return glob ? `the shared key input ${glob}` : '';
};

// `projects`: [{ name, root, subjects, targets: string[], metadata }] (the nodes of the Nx project graph).
// `opts.changed`: changed files relative to the repository root: records whose subject changed, or whose test files import a changed
// file (`opts.closureOf(projectRoot, file)`: the import closure of a test file, repository-relative), join the tag selection.
// `opts.sharedOf(file)`: why a file is a shared input (see sharedInput), or ''.
// Returns { tests: [{ project, file, tags, changed, dsp, targets }], unknown, shared: [{ file, why }] } where `targets` are the leaf targets of
// the file, one per device it runs on: `test-device--<file>--<dev>`, or `test-device--<dev>` for a project with one device test file.
// `shared` lists the changed files that no record's subject covers (or always, for the projects of every device task).
export const resolveSubset = (tags, projects, opts = {}) => {
  const { changed = [], closureOf, sharedOf } = opts;
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
        const hits = subjectFiles(rec, root, changed, rootOf, subjects);
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
  const shared = [];
  for (const file of changed) {
    const why = sharedOf?.(file) ?? '';
    if (why && (why.startsWith('the source of') || !covered.has(file))) shared.push({ file, why });
  }
  return { tests, unknown: tags.filter((tag) => !found.has(tag)), shared };
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

export const selectionPairs = (tests, projects) => {
  const known = new Set(projects.map((p) => p.name));
  const fixed = FIXED.filter((f) => known.has(f.project));
  return [...tests.flatMap((t) => t.targets.map((target) => ({ project: t.project, target }))), ...fixed];
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

// Why a `--base` selection must not run, or '': without tags, a selection must not be empty or silently partial.
export const baseProblem = ({ tags, base, changed, tests, shared }) => {
  if (!base || tags.length) return '';
  const advice = 'Name tags (test:subset -- S:... C:...) or run the full set.';
  if (!changed.length) return `no file changed since ${base}. ${advice}`;
  if (shared.length) {
    const [first] = shared;
    const more = shared.length > 1 ? ` (and ${shared.length - 1} more shared files)` : '';
    return `${first.file} is ${first.why}${more}: it reaches every device test. ${advice}`;
  }
  if (!tests.length)
    return `${changed.length} changed files since ${base} select no device test record (first: ${changed.slice(0, 3).join(', ')}). ${advice}`;
  return '';
};

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
    metadata: n.data.metadata,
  }));
};

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const run = args.includes('--run');
  const why = args.includes('--why');
  const at = args.indexOf('--base');
  const base = at >= 0 ? args[at + 1] : undefined;
  const tags = args.filter(
    (a, i) => a !== '--run' && a !== '--why' && a !== '--' && a !== '--base' && !(at >= 0 && i === at + 1),
  );
  if ((!tags.length && !base) || (at >= 0 && !base) || tags.some((t) => t.startsWith('-'))) {
    console.error(
      `usage: mise run test:subset -- [<tag...>] [--base REF] [--why] [--run]\ntags: ${vocabulary().join(' ')}`,
    );
    process.exit(2);
  }
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const projects = loadProjects(root);
  let opts = {};
  if (base) {
    const changed = changedFiles(root, base);
    const importInputs = testImportInputs(root);
    opts = {
      changed,
      closureOf: (projectRoot, file) => closureFiles(importInputs, `${projectRoot}/${file}`),
      sharedOf: (file) => sharedInput(file, projects),
    };
  }
  const { tests, unknown, shared } = resolveSubset(tags, projects, opts);
  const problem = baseProblem({ tags, base, changed: opts.changed ?? [], tests, shared });
  if (problem) {
    console.error(`test:subset: ${problem}`);
    process.exit(2);
  }
  if (shared.length && tags.length)
    console.log(`test:subset: note: ${shared.length} changed shared files; the tags decide`);
  if (unknown.length) {
    console.error(
      `test:subset: no device test has the tags ${unknown.join(' ')}\nvocabulary: ${vocabulary().join(' ')}`,
    );
    process.exit(2);
  }
  const pairs = selectionPairs(tests, projects);
  console.log(`test:subset: ${[...tags, ...(base ? [`--base ${base}`] : [])].join(' ')}`);
  for (const t of tests) {
    console.log(`  ${t.project} ${t.file}: ${t.targets.join(' ')}`);
    if (why) console.log(`    why: ${whyText(t)}`);
  }
  for (const f of FIXED) console.log(`  ${f.project}: ${f.target}`);
  const m = modeSummary(tests);
  console.log(
    `test:subset: ${m.count.sharc} coupled and ${m.count.zero} zero-DSP tasks; core budget ${m.cores}, ${m.slots} slots: ` +
      `${m.once.sharc} coupled and ${m.once.zero} zero-DSP start at once`,
  );
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

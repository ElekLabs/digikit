// `mise run test:subset -- [<tags...>] [--base REF] [--run]`: the device tests of the given record tags (tools/nx/acceptance-tags.mjs)
// as Nx leaf targets, plus gate-smoke and gate-pins. Without --run it prints them and how many start at once by DSP mode;
// with --run it runs them through `pnpm nx:pilot run-many` (DIGI_EMU_SLOTS and the other emulator settings pass through the environment).
// Tags of one call are a union: a test runs when any of its record tags is given.
// `--base REF` adds the tests whose subject changed since REF (`git diff REF`, with the working tree): a record's subject is the
// source of its project (not its tests) and of the projects of its `modules`, so a change to a machine selects the records that load it.
// Changes to shared inputs (SDK, profiles, patchkit) have no subject: name tags for them.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { AREAS, SERVICES } from './acceptance-tags.mjs';
import { budget, startAtOnce, workCores } from './resources.mjs';

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
const DEVICES = ['dt2', 'dn2'];

// Whether a changed file is production source of the project at `root`: below it, and not a test file, a Markdown file or its project.json.
const isSource = (file, root) =>
  file.startsWith(`${root}/`) &&
  !file.endsWith('.device.test.ts') &&
  !file.endsWith('.md') &&
  !file.endsWith('/project.json') &&
  !file.slice(root.length + 1).startsWith('tests/');

// Whether a record's subject changed: a changed source file of its project or of a project in `rec.modules`, or a changed
// test file the record lists. `rootOf`: project or module id -> root.
export const subjectChanged = (rec, own, changed, rootOf, subjects = []) => {
  const roots = [own, ...subjects, ...(rec.modules ?? [])].map((id) => rootOf.get(id) ?? id).filter(Boolean);
  const files = (rec.files ?? []).map((f) => `${own}/${f}`);
  return changed.some((file) => files.includes(file) || roots.some((root) => isSource(file, root)));
};

// `projects`: [{ name, root, subjects, targets: string[], metadata }] (the nodes of the Nx project graph).
// `changed`: changed files relative to the repository root (optional): records whose subject changed join the tag selection.
// Returns { tests: [{ project, file, tags, dsp, targets }], unknown } where `targets` are the leaf targets of the file, one per device
// it runs on: `test-device--<file>--<dev>`, or `test-device--<dev>` for a project with one device test file.
export const resolveSubset = (tags, projects, changed = []) => {
  const wanted = new Set(tags);
  const found = new Set();
  const tests = [];
  const rootOf = new Map(projects.map((p) => [p.name, p.root]));
  rootOf.set(undefined, undefined);
  for (const { name, root, subjects, targets, metadata } of projects) {
    const have = new Set(targets);
    for (const rec of metadata?.runtimeAcceptance ?? []) {
      const own = (rec.tags ?? []).filter((tag) => wanted.has(tag));
      const byPath = changed.length > 0 && root !== undefined && subjectChanged(rec, root, changed, rootOf, subjects);
      if (!own.length && !byPath) continue;
      own.forEach((tag) => found.add(tag));
      for (const file of rec.files ?? []) {
        const stem = stemOf(file);
        const leaves = DEVICES.flatMap((dev) =>
          [`test-device--${stem}--${dev}`, `test-device--${dev}`].filter((t) => have.has(t)).slice(0, 1),
        );
        if (leaves.length) tests.push({ project: name, file, tags: own, dsp: rec.dsp ?? 'zero', targets: leaves });
      }
    }
  }
  return { tests, unknown: tags.filter((tag) => !found.has(tag)) };
};

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
  const at = args.indexOf('--base');
  const base = at >= 0 ? args[at + 1] : undefined;
  const tags = args.filter((a, i) => a !== '--run' && a !== '--' && a !== '--base' && !(at >= 0 && i === at + 1));
  if ((!tags.length && !base) || (at >= 0 && !base) || tags.some((t) => t.startsWith('-'))) {
    console.error(`usage: mise run test:subset -- [<tag...>] [--base REF] [--run]\ntags: ${vocabulary().join(' ')}`);
    process.exit(2);
  }
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const projects = loadProjects(root);
  let changed = [];
  if (base) {
    const diff = spawnSync('git', ['diff', '--name-only', base], { cwd: root, encoding: 'utf8' });
    if (diff.status !== 0) throw new Error(`git diff ${base} failed:\n${diff.stderr}`);
    changed = diff.stdout.split('\n').filter(Boolean);
  }
  const { tests, unknown } = resolveSubset(tags, projects, changed);
  if (unknown.length) {
    console.error(
      `test:subset: no device test has the tags ${unknown.join(' ')}\nvocabulary: ${vocabulary().join(' ')}`,
    );
    process.exit(2);
  }
  const pairs = selectionPairs(tests, projects);
  console.log(`test:subset: ${[...tags, ...(base ? [`--base ${base}`] : [])].join(' ')}`);
  for (const t of tests) console.log(`  ${t.project} ${t.file}: ${t.targets.join(' ')}`);
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

// Ownership-based device test selection of the merge gate (`test-subset.mjs --gate`, docs/TESTING.md "Area-based device selection").
// A device test is a mechanism test on fixtures (owned by the mechanism's project) or a behaviour test (owned by its service or module).
// A changed file selects tests by what owns it, with no area tags and no list of ours:
//   1. A device file (devices/<stem>.toml, its companions) names a device: the halves of that device only (`devices` of the result).
//   2. A firmware declaration `ports/fw/<point>.toml` touches the projects of its `users`.
//   3. Any other file belongs to the project with the deepest root containing it (Nx's own rule, from the `nx graph` nodes: nested
//      projects win over their parents). Markdown and project.json files select nothing; a test source (config/test-source-patterns.json)
//      does not touch its project, it selects the tests that import it (rule 5).
//   4. A touched project P selects the device tests of P itself, of every project whose authored `implicitDependencies` name P (a test
//      project names its subject there) and of every record whose `modules` list P when P is a fixture project (tests/fixtures/modules/**); a loaded service or real module is not a subject.
//   5. A changed TypeScript file (the driver packages only for DRIVER_SUBJECT_FILES, as the device keys do) selects the device test files whose import closure holds it (a harness or test-support helper),
//      a changed test file or a file in a record's `sources` selects that record.
//   6. A changed path owned by no project, or by a project no test names, selects nothing and is returned in `unowned` / `quiet`,
//      for the gate to print.
// Conformance (`ownedConformance`): the modules whose own source changed. Nothing here forces a wide selection; the full tier
// (`--all-affected`, `test:full`) stays for that.
import { DRIVER_ROOTS, DRIVER_SUBJECT_FILES } from './device-test-keys.mjs';
import { isTestSource } from './test-source.mjs';

export const DEVICES = ['dt2', 'dn2', 'st'];

export const stemOf = (file) =>
  file
    .split('/')
    .at(-1)
    .replace(/\.device\.test\.ts$/, '');

/** The project with the deepest root containing `file` (a `.` root owns nothing), or undefined. `projects`: [{ name, root }]. */
export const ownerOf = (file, projects) => {
  let best;
  for (const p of projects) {
    if (p.root === undefined || p.root === '.' || p.root === '') continue;
    if (file !== p.root && !file.startsWith(`${p.root}/`)) continue;
    if (!best || p.root.length > best.root.length) best = p;
  }
  return best;
};

// Markdown, project.json and a file below a test source rule select nothing as product of their project.
const isProduct = (file, root) => {
  const rel = file.slice(root.length + 1);
  return (
    !file.endsWith('.device.test.ts') && !file.endsWith('.md') && !file.endsWith('/project.json') && !isTestSource(rel)
  );
};

// The leaf targets of a record file, one per device: `test-device--<file>--<dev>`, or `test-device--<dev>` for a one-file project.
export const leavesOf = (file, targets) => {
  const have = new Set(targets);
  const stem = stemOf(file);
  return DEVICES.flatMap((dev) =>
    [`test-device--${stem}--${dev}`, `test-device--${dev}`].filter((t) => have.has(t)).slice(0, 1),
  );
};

// The names a record's subject is made of: its project, the authored implicit dependencies of the project, the modules it loads.
// A loaded module selects the test only when it is a fixture (tests/fixtures/modules/**): a service or real module a test loads is
// not its subject (most tests load machines.dks), the test names its subject in implicitDependencies.
const FIXTURE_ROOT = 'tests/fixtures/modules/';
const subjectNames = (project, rec, byName) =>
  new Set([
    project.name,
    ...(project.subjects ?? []),
    ...(rec.modules ?? []).filter((m) => byName.get(m)?.root?.startsWith(FIXTURE_ROOT)),
  ]);

/** Changed paths -> the device tests they select.
 * `projects`: [{ name, root, subjects (authored implicitDependencies), targets, metadata }] (the nodes of the Nx graph).
 * `opts.changed`: repository-relative changed files. `opts.closureOf(projectRoot, file)`: the import closure of a test file.
 * `opts.portUsers(file)`: project names of a firmware declaration file. `opts.deviceOf(file)`: the device a device file names.
 * Returns { tests: [{ project, file, named, changed, dsp, targets }], mapped: [{ file, project }], devices: string[], unowned:
 * [{ file, why }], quiet: [{ file, project }] (owned by a project no test names), covered: Set }. */
export const ownershipSelection = (projects, { changed, closureOf, portUsers, deviceOf }) => {
  const byName = new Map(projects.map((p) => [p.name, p]));
  const touched = new Map(); // project name -> files
  const mapped = [];
  const unowned = [];
  const devices = new Set();
  const touch = (name, file) => touched.set(name, [...(touched.get(name) ?? []), file]);
  for (const file of changed) {
    const device = deviceOf?.(file);
    if (device) {
      devices.add(device);
      mapped.push({ file, project: `device ${device}` });
      continue;
    }
    if (/^ports\/fw\/[^/]+\.toml$/.test(file)) {
      const users = portUsers?.(file) ?? [];
      const known = users.filter((u) => byName.has(u));
      for (const u of known) touch(u, file);
      for (const u of users.filter((u) => !byName.has(u)))
        unowned.push({ file, why: `user ${u} of the port file is no project` });
      if (!users.length) unowned.push({ file, why: 'the port file names no users' });
      known.forEach((u) => mapped.push({ file, project: u }));
      continue;
    }
    const owner = ownerOf(file, projects);
    if (!owner) {
      if (!file.endsWith('.md')) unowned.push({ file, why: 'no project owns it' });
      continue;
    }
    mapped.push({ file, project: owner.name });
    if (isProduct(file, owner.root)) touch(owner.name, file);
  }
  // The driver packages have host tests of their own and leave the device keys, except the protocol and log layout files.
  const tsChanged = changed.filter(
    (f) => f.endsWith('.ts') && (DRIVER_SUBJECT_FILES.includes(f) || !DRIVER_ROOTS.some((r) => f.startsWith(r))),
  );
  const covered = new Set();
  const tests = [];
  for (const project of projects) {
    const { name, root, targets } = project;
    for (const rec of project.metadata?.runtimeAcceptance ?? []) {
      const via = new Set();
      for (const [p, files] of touched) if (subjectNames(project, rec, byName).has(p)) files.forEach((f) => via.add(f));
      const own = [...(rec.files ?? []).map((f) => `${root}/${f}`), ...(rec.sources ?? [])];
      changed.filter((f) => own.includes(f)).forEach((f) => via.add(f));
      if (closureOf && tsChanged.length)
        for (const f of rec.files ?? []) {
          const closure = new Set(closureOf(root, f));
          tsChanged.filter((c) => closure.has(c)).forEach((c) => via.add(c));
        }
      if (!via.size) continue;
      via.forEach((f) => covered.add(f));
      for (const file of rec.files ?? []) {
        const leaves = leavesOf(file, targets);
        if (leaves.length) {
          tests.push({ project: name, file, named: [], changed: [...via], dsp: rec.dsp ?? 'zero', targets: leaves });
        }
      }
    }
  }
  // A touched project no device test names: printed, because a silent drop looks like a pass.
  const quiet = [...touched].flatMap(([project, files]) =>
    files.filter((f) => !covered.has(f)).map((file) => ({ file, project })),
  );
  return { tests, mapped, devices: [...devices], unowned, quiet, covered };
};

/** The `project:test-conformance` tasks among `affected` of the modules whose own source changed (deepest owner, product files). */
export const ownedConformance = (affected, projects, changed) => {
  const roots = new Map(projects.map((p) => [p.name, p.root]));
  const owned = new Set(
    changed
      .map((file) => ownerOf(file, projects))
      .filter(Boolean)
      .map((p) => p.name),
  );
  return [...affected].filter((id) => {
    if (!id.endsWith(':test-conformance')) return false;
    const name = id.slice(0, -':test-conformance'.length);
    const root = roots.get(name);
    return (
      root !== undefined &&
      owned.has(name) &&
      changed.some((f) => ownerOf(f, projects)?.name === name && isProduct(f, root))
    );
  });
};

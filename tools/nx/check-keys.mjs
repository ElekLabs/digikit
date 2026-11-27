// Check (the `device-keys` target of the workspace project, which `lint` depends on): the cache key of every device-test half names the files the
// test reads, so an edit of one of them cannot leave a cached result in place.
// On the resolved project graph (`nx graph`), for each `test-device--<file>--<device>` and `test-device-all--<file>--<device>` target:
//  - the test file itself is an input;
//  - each path its runtimeAcceptance record lists in `sources` is an input (as written, or covered by a glob of the key; a driver package
//    file counts only when it holds device facts, like the imports below);
//  - each file in the relative-import closure of the test file is an input, except the driver packages that only the full key holds
//    (`withoutDriver` in tools/nx/device-test-keys.mjs; the files that hold device facts stay).
// It also checks that the devices a runtimeAcceptance record declares are the ones the test source pins, and that the test-source exclusions of nx.json and of `subjectInputs` are the ones the rule derives.
// The closure here is read with a plain regular expression, not with `tools/nx/ts-imports.mjs` that builds the key, so a fault of that
// reader shows. Run by hand: `node tools/nx/check-keys.mjs`.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, matchesGlob, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { subjectInputs, withoutDriver } from './device-test-keys.mjs';
import { DEVICES } from './device-owners.mjs';
import { testSourceGlobs } from './test-source.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const WS = '{workspaceRoot}/';

const dir = mkdtempSync(join(tmpdir(), 'check-keys-'));
let nodes;
try {
  const file = join(dir, 'graph.json');
  execFileSync('node', ['tools/nx/nx.mjs', 'graph', `--file=${file}`], {
    cwd: root,
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  nodes = JSON.parse(readFileSync(file, 'utf8')).graph.nodes;
} finally {
  rmSync(dir, { recursive: true, force: true });
}

const IMPORT = /(?:\bfrom\s*|\bimport\s*\(?\s*)['"](\.{1,2}\/[^'"]+)['"]/g;
const resolveImport = (from, spec) => {
  const base = resolve(dirname(from), spec.replace(/\.js$/, ''));
  return [base, `${base}.ts`, `${base}.tsx`, join(base, 'index.ts')].find((p) => existsSync(p) && /\.tsx?$/.test(p));
};
// The files a TypeScript file reaches through relative imports (the file itself included), as paths relative to the repository.
const closure = (start) => {
  const seen = new Set();
  const visit = (abs) => {
    if (seen.has(abs)) return;
    seen.add(abs);
    for (const m of readFileSync(abs, 'utf8').matchAll(IMPORT)) {
      const next = resolveImport(abs, m[1]);
      if (next) visit(next);
    }
  };
  visit(start);
  return [...seen].map((p) => relative(root, p));
};

const covers = (inputs, project, rel) => {
  const globs = inputs
    .filter((i) => typeof i === 'string' && i.includes('/'))
    .map((i) => i.replace('{projectRoot}', project));
  const test = (g) => matchesGlob(rel, g.replace(WS, '').replace(/^!/, ''));
  return globs.filter((g) => g.startsWith(WS)).some(test) && !globs.filter((g) => g.startsWith(`!${WS}`)).some(test);
};

const problems = [];
let checked = 0;

// The static `production` input of nx.json cannot import the test-source rule (config/test-source-patterns.json): its exclusions are the
// derived list, nothing missing, nothing extra; `subjectInputs` of a device test derives the same list for the files of the subject.
const same = (what, have, want) => {
  const missing = [...want].filter((g) => !have.includes(g));
  const extra = have.filter((g) => !want.has(g));
  if (missing.length || extra.length || have.length !== new Set(have).size)
    problems.push(`${what}: missing ${JSON.stringify(missing)}, extra ${JSON.stringify(extra)}`);
};
const production = JSON.parse(readFileSync(join(root, 'nx.json'), 'utf8')).namedInputs.production;
same(
  'nx.json namedInputs.production',
  production.filter((g) => g.startsWith('!')),
  new Set(['!{projectRoot}/**/*.md', ...testSourceGlobs('{projectRoot}')]),
);
same(
  'subjectInputs',
  subjectInputs('modules/x', root).filter((g) => g.startsWith('!') && g.includes('/**/') && !g.endsWith('.md')),
  new Set(testSourceGlobs('{workspaceRoot}/modules/x')),
);
for (const [name, node] of Object.entries(nodes)) {
  const records = node.data.metadata?.runtimeAcceptance ?? [];
  const project = node.data.root;
  for (const record of records) {
    for (const file of record.files ?? []) {
      const test = join(project, file);
      const stem = file.replace(/^.*\//, '').replace(/\.device\.test\.ts$/, '');
      const wanted = [
        test,
        ...(existsSync(join(root, test)) ? closure(join(root, test)) : []),
        ...(record.sources ?? []),
      ]
        .map((p) => `${WS}${p}`)
        .filter((p, i, all) => all.indexOf(p) === i);
      // the driver packages are left out of the merge and narrow keys by design, also when a record lists one in `sources`
      const wantedPaths = withoutDriver(wanted).map((p) => p.slice(WS.length));
      for (const target of Object.keys(node.data.targets)) {
        if (!new RegExp(`^test-device(-all)?--${stem.replace(/\W/g, '\\W')}--[a-z0-9]+$`).test(target)) continue;
        checked++;
        const inputs = node.data.targets[target].inputs ?? [];
        for (const path of wantedPaths) {
          const literal = inputs.includes(`${WS}${path}`);
          if (!literal && !covers(inputs, project, path)) problems.push(`${name}:${target}: ${path} is no input`);
        }
      }
    }
  }
}

// The devices a record declares agree with what the test source pins: a file that the source pins to one device and the record leaves on both
// queues an empty half, and a record that drops a device the source runs loses that coverage.
const pinned = (source) => {
  const found = new Set();
  const names = DEVICES.join('|');
  for (const m of source.matchAll(new RegExp(`devices: \\['(${names})'\\]|describeOn\\('(${names})'`, 'g')))
    found.add(m[1] ?? m[2]);
  // the SHOT files of a device run through a helper that pins it (`<device>Tests(`)
  for (const d of DEVICES) if (new RegExp(`\\b${d}Tests\\(`).test(source)) found.add(d);
  return found;
};
const testFiles = execFileSync('git', ['ls-files', '*.device.test.ts'], { cwd: root, encoding: 'utf8' })
  .split('\n')
  .filter((f) => f && !f.endsWith('.manual.device.test.ts'));
for (const file of testFiles) {
  let dir = file;
  let project;
  for (;;) {
    dir = dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/')) : '';
    try {
      project = JSON.parse(readFileSync(join(root, dir, 'project.json'), 'utf8'));
      break;
    } catch {
      if (!dir) break;
    }
  }
  const rel = file.slice(dir.length + 1);
  const declared = (project?.metadata?.runtimeAcceptance ?? []).find((rec) => rec.files?.includes(rel))?.devices;
  const source = readFileSync(join(root, file), 'utf8');
  const pins = pinned(source);
  // a file whose source pins a device in only some tests, or several devices, is left on both
  const expected = pins.size === 1 && !/describeOn\(\s*\w+\./.test(source) ? [...pins] : undefined;
  if (JSON.stringify(declared?.length === 2 ? undefined : declared) !== JSON.stringify(expected))
    problems.push(
      `${file}: its source pins ${JSON.stringify([...pins])}, its record declares devices ${JSON.stringify(declared)}`,
    );
}

if (problems.length) {
  console.error(`check-keys: ${problems.length} problem(s) in the cache keys:`);
  for (const p of problems.slice(0, 40)) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`check-keys: the keys of ${checked} device-test halves name the test, its sources and its imports`);

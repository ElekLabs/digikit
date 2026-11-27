// Runtime areas of the merge gate (docs/TESTING.md "Runtime areas"). The data is tools/nx/runtime-areas.json: a changed runtime source
// file that matches an area selects the device tests of that area instead of every test that names the owning project; a runtime file
// in no area and not host-proven is an error (`checkComplete`; there is no full-suite fallback). Used by test-subset.mjs `--gate`.
import { spawnSync } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import { matchesGlob } from 'node:path';
import { fileURLToPath } from 'node:url';
import { leavesOf } from './device-owners.mjs';

const DATA = fileURLToPath(new URL('./runtime-areas.json', import.meta.url));

export const loadAreas = (file = DATA) => JSON.parse(readFileSync(file, 'utf8'));

const any = (file, globs = []) => globs.some((g) => matchesGlob(file, g));

/** How a file is governed: { kind: 'area', area } | { kind: 'host-proven' } | { kind: 'unclassified' } | undefined (not a runtime-area file). */
export const classify = (file, areas) => {
  const governed = any(file, areas.scope) && !any(file, areas.exclude);
  if (governed && any(file, areas.testSources)) return { kind: 'host-proven' };
  const named = Object.entries(areas.areas).find(([, a]) => any(file, a.globs));
  if (named) return { kind: 'area', area: named[0] };
  if (!any(file, areas.scope) || any(file, areas.exclude)) return undefined;
  return any(file, areas.hostProven) ? { kind: 'host-proven' } : { kind: 'unclassified' };
};

/** The test records of a 'project:file' / 'project:*' entry: [{ project, file, dsp, targets }]; [] when it names none. */
const recordsOf = (entry, projects) => {
  const at = entry.indexOf(':');
  const [name, file] = [entry.slice(0, at), entry.slice(at + 1)];
  const project = projects.find((p) => p.name === name);
  const out = [];
  for (const rec of project?.metadata?.runtimeAcceptance ?? [])
    for (const f of rec.files ?? []) {
      if (file !== '*' && file !== f) continue;
      const targets = leavesOf(f, project.targets);
      if (targets.length) out.push({ project: name, file: f, dsp: rec.dsp ?? 'zero', targets });
    }
  return out;
};

/** Entries of the map that name no test: [{ area, entry }]. The host test keeps the map true to the project records. */
export const danglingEntries = (areas, projects) =>
  Object.entries(areas.areas).flatMap(([area, a]) =>
    a.tests.filter((entry) => !recordsOf(entry, projects).length).map((entry) => ({ area, entry })),
  );

/** Changed files -> { tests, governed: Set of files the areas decided, notes: lines the gate prints }.
 * `tests` are { project, file, named: [], changed: [files], dsp, targets } as namedSelection returns. */
export const areaSelection = (projects, changed, areas = loadAreas()) => {
  const byKey = new Map();
  const add = (rec, file, via) => {
    const key = `${rec.project}:${rec.file}`;
    const t = byKey.get(key) ?? { ...rec, named: [], changed: [] };
    t.named = [...new Set([...t.named, via])];
    t.changed = [...new Set([...t.changed, file])];
    byKey.set(key, t);
  };
  const governed = new Set();
  const notes = [];
  const hit = new Map(); // area -> files
  const unclassified = [];
  for (const file of changed) {
    const c = classify(file, areas);
    if (!c) continue;
    governed.add(file);
    if (c.kind === 'area') {
      hit.set(c.area, [...(hit.get(c.area) ?? []), file]);
      for (const entry of areas.areas[c.area].tests)
        for (const rec of recordsOf(entry, projects)) add(rec, file, `area ${c.area}`);
    } else if (c.kind === 'unclassified') unclassified.push(file);
  }
  for (const [area, files] of hit) notes.push(`area ${area}: ${files.length} file(s), e.g. ${files[0]}`);
  return { tests: [...byKey.values()], governed, notes, unclassified };
};

/** The files among `files` under the runtime scope that are in no area and not host-proven. */
export const checkComplete = (files, areas = loadAreas()) =>
  files.filter((f) => classify(f, areas)?.kind === 'unclassified');

if (
  process.argv[1] &&
  realpathSync(process.argv[1]) === fileURLToPath(import.meta.url) &&
  process.argv[2] === '--check'
) {
  const root = fileURLToPath(new URL('../..', import.meta.url));
  const files = spawnSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], {
    cwd: root,
    encoding: 'utf8',
  })
    .stdout.split('\n')
    .filter(Boolean);
  const bad = checkComplete(files);
  if (bad.length) {
    console.error(
      `runtime-areas: ${bad.length} runtime file(s) in no area and not host-proven (add them to tools/nx/runtime-areas.json):`,
    );
    for (const f of bad.slice(0, 30)) console.error(`  ${f}`);
    process.exit(1);
  }
  console.log('runtime-areas: every runtime file is in an area or host-proven');
}

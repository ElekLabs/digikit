// Check (the `runtime-areas` target of the workspace project, which `lint` depends on): the runtime area map (tools/nx/runtime-areas.json)
// is true to the repository.
//  - every tracked runtime file is in an area or host-proven (`node tools/nx/runtime-areas.mjs --check` says the same for one purpose);
//  - every test the map names is a device test of a project in the resolved Nx project graph, so no area selects nothing by a typo or after a rename.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { checkComplete, danglingEntries, loadAreas } from './runtime-areas.mjs';
import { loadProjects } from './test-subset.mjs';

const root = fileURLToPath(new URL('../..', import.meta.url));
const areas = loadAreas();
const files = spawnSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], {
  cwd: root,
  encoding: 'utf8',
})
  .stdout.split('\n')
  .filter(Boolean);
const unclassified = checkComplete(files, areas);
const dangling = danglingEntries(areas, loadProjects(root));
for (const f of unclassified.slice(0, 30))
  console.error(`runtime-areas: ${f} is in no area and not host-proven (add it to tools/nx/runtime-areas.json)`);
for (const d of dangling)
  console.error(`runtime-areas: area ${d.area} names ${d.entry}, which is no device test of the project graph`);
if (unclassified.length || dangling.length) process.exit(1);
console.log('runtime-areas: every runtime file is classified and every named test exists');

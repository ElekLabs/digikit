// Check (run by the `vitest-host` target of the workspace project, which `lint` depends on): each Vitest project of
// vitest.host/NAME.json is named in the inputs of every target that runs `vitest ... --project NAME`, so the key of a host test
// follows its own entry and no other. The targets come from the resolved project graph (`nx graph`), so targets that plugins and
// tools/nx/*.mjs generate (module tests, rig tests) are checked like those written in a project.json.
//  - a target that runs `--project NAME` without `{workspaceRoot}/vitest.host/NAME.json` in its inputs fails;
//  - a target that lists another project's file fails;
//  - a vitest.host file that no target runs, or whose `name` differs from its file name, fails.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const PREFIX = '{workspaceRoot}/vitest.host/';
const RUN = /\bvitest\b[^"]*?--project[ =]([\w-]+)/g;

const entries = readdirSync(join(root, 'vitest.host'))
  .filter((f) => f.endsWith('.json'))
  .map((f) => f.slice(0, -'.json'.length));

const dir = mkdtempSync(join(tmpdir(), 'vitest-host-'));
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

const problems = [];
const run = new Set();
for (const [project, node] of Object.entries(nodes)) {
  for (const [name, target] of Object.entries(node.data.targets ?? {})) {
    const text = JSON.stringify([target.options ?? {}, target.metadata?.scriptContent ?? '']);
    if (!text.includes('vitest.host.config.ts')) continue; // another Vitest config (tests/regression) has its own projects
    const names = [...new Set([...text.matchAll(RUN)].map((m) => m[1]))];
    if (names.length === 0) continue;
    const at = `${project}:${name}`;
    const inputs = (target.inputs ?? []).filter((i) => typeof i === 'string');
    const own = names.map((n) => `${PREFIX}${n}.json`);
    for (const n of names) {
      run.add(n);
      if (!entries.includes(n)) problems.push(`${at}: runs unknown Vitest project ${n}`);
      else if (!inputs.includes(`${PREFIX}${n}.json`)) problems.push(`${at}: inputs must list ${PREFIX}${n}.json`);
    }
    for (const i of inputs)
      if (i.startsWith(PREFIX) && !own.includes(i)) problems.push(`${at}: lists ${i}, which it does not run`);
  }
}
for (const n of entries) {
  if (!run.has(n)) problems.push(`vitest.host/${n}.json: no target runs project ${n}`);
  const declared = JSON.parse(readFileSync(join(root, 'vitest.host', `${n}.json`), 'utf8')).name;
  if (declared !== n) problems.push(`vitest.host/${n}.json: name is ${declared}`);
}
if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log(`vitest.host: ${entries.length} projects, each named by the targets that run it`);

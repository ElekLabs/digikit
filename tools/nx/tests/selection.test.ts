import { execFileSync } from 'node:child_process';
import { existsSync, globSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createDependencies } from '../modules.mjs';
import { manifestClosure } from '../module-manifests.mjs';
import { TIERS } from '../gate.mjs';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), 'digikit-nx-selection-'));
afterAll(() => rmSync(scratch, { recursive: true, force: true }));

type TaskGraph = {
  tasks: Record<string, { target: { project: string; target: string } }>;
  dependencies: Record<string, { id: string }[]>;
};

type ProjectGraph = {
  nodes: Record<
    string,
    {
      data: {
        root: string;
        tags?: string[];
        targets: Record<
          string,
          {
            options?: { command?: string };
            inputs?: unknown[];
            cache?: boolean;
          }
        >;
        metadata?: { runtimeAcceptance?: { files?: string[]; sources?: string[] }[] };
      };
    }
  >;
  dependencies: Record<string, { target: string }[]>;
};

function nx(args: string[]): string {
  const env: Record<string, string | undefined> = {
    ...process.env,
    NX_DAEMON: 'false',
    NX_NO_CLOUD: 'true',
    NX_ISOLATE_PLUGINS: 'false',
    NX_WORKSPACE_DATA_DIRECTORY: join(scratch, 'workspace-data'),
  };
  delete env.NX_LEGACY_AFFECTED;
  return execFileSync('pnpm', ['exec', 'nx', ...args], {
    cwd: root,
    env,
    // hang guard only: a normal call returns in seconds to tens of seconds, even on a loaded host; this catches
    // a hung nx and is never a wait
    timeout: 600_000,
    stdio: 'pipe',
    encoding: 'utf8',
  });
}

function plan(args: string[], outputFlag = '--graph'): { tasks: TaskGraph; graph: ProjectGraph } {
  const output = join(scratch, 'tasks.json');
  nx([...args, `${outputFlag}=${output}`]);
  return JSON.parse(readFileSync(output, 'utf8'));
}

const selected = (file: string, targets: string) => {
  expect(existsSync(join(root, file)), `selection fixture exists: ${file}`).toBe(true);
  return plan(['affected', `--files=${file}`, `--targets=${targets}`]).tasks;
};

let workspace: ProjectGraph;
// One cold project-graph build per nx() call (about 3-5 s idle, 10x that on a loaded gate host). The limits
// here (10 min per nx call, a little more per test) are hang guards only: they catch a hung nx and never act
// as a wait. A 30 s limit failed five cases on a busy host because it measured host load.
beforeAll(() => {
  workspace = plan(['graph'], '--file').graph;
}, 660_000);

// Let Vitest flush worker updates between synchronous Nx subprocesses.
afterEach(async () => {
  await new Promise<void>((resolve) => setImmediate(resolve));
});

function imageConsumer(): string {
  return Object.entries(workspace.nodes).find(
    ([, node]) => node.data.root.startsWith('modules/machine/') && node.data.targets['test-conformance'],
  )![0];
}

function otherImageConsumer(): string {
  return Object.entries(workspace.nodes).find(
    ([id, node]) =>
      id !== imageConsumer() && node.data.root.startsWith('modules/machine/') && node.data.targets['test-conformance'],
  )![0];
}

it('keeps every gated runtime device file in exactly one owner command', () => {
  const base = workspace.nodes['core/runtime-tests'].data;
  const owners = Object.values(workspace.nodes)
    .map((node) => node.data)
    .filter(
      (owner) =>
        owner.tags?.includes('test-only') && (owner.root === base.root || owner.root.startsWith(base.root + '/')),
    );
  const actual = owners
    .flatMap((owner) => {
      const targets = Object.entries(owner.targets)
        .filter(
          ([name, target]) =>
            /^test-device(-full)?(--|$)/.test(name) &&
            // one half per file: the DT2 half, or the DN2 half of a file that has no DT2 half
            !(name.endsWith('--dn2') && owner.targets[name.replace(/--dn2$/, '--dt2')]) &&
            target.options?.command,
        )
        .map(([, target]) => target);
      const files = targets.flatMap((target) =>
        (target.options.command.match(/\S+\.device\.test\.ts/g) ?? []).map((file) =>
          resolve(root, 'tests/regression', file),
        ),
      );
      const inputs = targets.flatMap((target) => target.inputs);
      const sources = new Set((owner.metadata?.runtimeAcceptance ?? []).flatMap((rec) => rec.sources ?? []));
      for (const source of sources) expect(inputs).toContain(`{workspaceRoot}/${source}`);
      for (const rec of owner.metadata?.runtimeAcceptance ?? [])
        for (const file of rec.files ?? []) expect(files).toContain(join(root, owner.root, file));
      return files;
    })
    .sort();
  const expected = globSync('**/*.device.test.ts', { cwd: join(root, base.root) })
    .filter((file) => !file.endsWith('.manual.device.test.ts'))
    .map((file) => join(root, base.root, file))
    .sort();
  expect(actual).toEqual(expected);
});

it('derives content runtime edges from the unique runtime role', () => {
  const fixture = mkdtempSync(join(scratch, 'runtime-role-'));
  const projects = Object.fromEntries(
    [
      ['renamed/runtime', 'runtime/renamed', 'runtime'],
      ['example/content', 'relocated/tools/content', undefined],
    ].map(([name, dir, role]) => {
      mkdirSync(join(fixture, dir!), { recursive: true });
      writeFileSync(
        join(fixture, dir!, 'module.toml'),
        `[module]\nid = "${name}"\ntype = "service"\n${role ? `role = "${role}"\n` : ''}`,
      );
      return [name, { name, root: dir, tags: role ? ['module'] : ['module', 'release-module'] }];
    }),
  );
  const dependencies = createDependencies({}, { workspaceRoot: fixture, projects });
  expect(dependencies.filter((dep) => dep.source === 'example/content').map((dep) => dep.target)).toEqual([
    'renamed/runtime',
  ]);
  writeFileSync(
    join(fixture, 'relocated/tools/content/module.toml'),
    '[module]\nid = "example/content"\nrole = "runtime"\n',
  );
  expect(() => createDependencies({}, { workspaceRoot: fixture, projects })).toThrow(
    'Expected one module with role=runtime, found 2',
  );
});

it('forms explicit manifest libraries from selected modules and transitive requirements only', () => {
  const manifest = (id: string, requires: string[] = []) =>
    [id, { file: `modules/${id}/module.toml`, manifest: { requires } }] as const;
  const modules = new Map([
    manifest('example/content', ['example/adapter']),
    manifest('example/adapter', ['example/runtime']),
    manifest('example/runtime'),
    manifest('example/unrelated'),
  ]);
  expect(manifestClosure(modules, ['example/content', 'example/adapter'])).toEqual([
    'modules/example/adapter/module.toml',
    'modules/example/content/module.toml',
    'modules/example/runtime/module.toml',
  ]);
  expect(() => manifestClosure(modules, ['example/missing'])).toThrow('Unknown scoped module example/missing');
});

describe('workspace selection', () => {
  it('retains core manifest selection for producers and their image consumers', () => {
    const graph = selected('runtime/core/module.toml', 'build-objects,test-conformance');
    expect(graph.tasks['core/runtime:build-objects']).toBeDefined();
    expect(graph.tasks[`${imageConsumer()}:test-conformance`]).toBeDefined();
    expect(graph.tasks[`${otherImageConsumer()}:test-conformance`]).toBeDefined();
  }, 660_000);

  it('packs affected modules without device tests through the default gate', () => {
    const targets = TIERS.merge.targets.join(',');
    const [owner, node] = Object.entries(workspace.nodes).find(
      ([, node]) =>
        node.data.targets.pack && !node.data.targets['test-device'] && !node.data.targets['test-conformance'],
    )!;
    const source = globSync('src/**/*.{c,rs}', { cwd: join(root, node.data.root) }).sort()[0];
    expect(source).toBeDefined();
    const graph = selected(`${node.data.root}/${source}`, targets);
    expect(graph.tasks[`${owner}:pack`]).toBeDefined();
    expect(node.data.targets.pack.cache).toBe(false);
  }, 660_000);

  it('selects image consumers when ui.c changes', () => {
    const graph = selected('runtime/platform/ui/src/ui.c', 'test-device,test-conformance');
    expect(graph.tasks['core/runtime-tests:test-device']).toBeDefined();
    expect(graph.tasks[`${imageConsumer()}:test-conformance`]).toBeDefined();
  }, 660_000);

  it('selects hermetic Rust tests when the release profile digest recipe changes', () => {
    const graph = selected('tools/nx/cargo-profile-env.mjs', 'rust-tests');
    expect(graph.tasks['dkneeds:rust-tests']).toBeDefined();
    expect(graph.tasks['device-profile:rust-tests']).toBeDefined();
    expect(graph.tasks['patchkit:rust-tests']).toBeUndefined();
  }, 660_000);

  it.each(['contract.rs', 'lib.rs'])(
    'selects Rust tests of Cargo consumers when shared %s changes',
    (file) => {
      const support = 'runtime/core/test-support';
      const cargo = (dir: string) =>
        parse(readFileSync(join(root, dir, 'Cargo.toml'), 'utf8')) as {
          package?: { name: string };
          workspace?: { members: string[] };
          'dev-dependencies'?: Record<string, string | { path?: string }>;
        };
      const usesSupport = (dir: string) =>
        Object.values(cargo(dir)['dev-dependencies'] ?? {}).some(
          (dep) => typeof dep === 'object' && dep.path && resolve(root, dir, dep.path) === join(root, support),
        );
      const workspaceConsumers = cargo('.')
        .workspace!.members.filter(usesSupport)
        .map((dir) => cargo(dir).package!.name);
      const standaloneConsumers = globSync('services/*/dks/Cargo.toml', { cwd: root })
        .map(dirname)
        .filter(usesSupport)
        .map((dir) => {
          const { module } = parse(readFileSync(join(root, dir, 'module.toml'), 'utf8')) as { module: { id: string } };
          return module.id;
        });
      const runtime = 'runtime/core';
      expect(workspaceConsumers).toContain('dkrt-svc-shortcuts');
      expect(standaloneConsumers).toContain('core/services/machines');
      expect(usesSupport(`${runtime}/src/rs`)).toBe(true);
      expect(TIERS.merge.targets).toContain('rust-tests');
      const graph = selected(`${support}/src/${file}`, 'rust-tests');
      for (const owner of [...workspaceConsumers, ...standaloneConsumers, 'core/runtime']) {
        const target = `${owner}:rust-tests`;
        expect(graph.tasks[target], `${file} selects ${target}`).toBeDefined();
      }
      expect(graph.tasks['dkbulk:rust-tests']).toBeUndefined();
    },
    660_000,
  );

  it('routes a module DSP edit to its own rig tests', () => {
    const project = workspace.nodes['digikit/pluck'].data;
    const graph = selected(`${project.root}/src/dsp/pluck.s`, 'pack,test-rig,test-conformance-rig-merge');
    expect(
      Object.keys(graph.tasks)
        .filter((id) => /:test-(conformance-rig-merge|rig)$/.test(id))
        .sort(),
    ).toEqual(['digikit/pluck:test-conformance-rig-merge', 'digikit/pluck:test-rig']);
    expect(graph.tasks['digikit/pluck:pack']).toBeDefined();
  }, 660_000);

  it('keeps shared SDK contract edits on the SDK host, pack, runtime device and conformance selection', () => {
    const graph = selected('sdk/include/dk/core/module.h', 'test-host,pack,test-device,test-conformance');
    for (const target of [
      'sdk:test-host',
      `${imageConsumer()}:pack`,
      'core/runtime-tests:test-device',
      `${imageConsumer()}:test-conformance`,
    ])
      expect(graph.tasks[target], target).toBeDefined();
  }, 660_000);
});

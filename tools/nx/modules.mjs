// Nx plugin: one project per module.toml. Edges: [module].requires plus [[object]].sources paths
// that resolve into another module directory. tools/devtools/verify_route.py reads them from the graph.
import { globSync, readFileSync } from 'node:fs';
import { dirname, join, normalize } from 'node:path';
import { DependencyType } from '@nx/devkit';
import { parse } from 'smol-toml';

const read = (root, toml) => parse(readFileSync(join(root, toml), 'utf8'));

export const createNodesV2 = [
  '{modules,tests/fixtures/modules}/**/module.toml',
  (files, _opts, ctx) =>
    files.map((f) => {
      const { module: m } = read(ctx.workspaceRoot, f);
      const root = dirname(f);
      const project = { name: m.id, root, tags: ['module', `type:${m.type}`] };
      const hostTests = globSync('tests/**/*.test.ts', { cwd: join(ctx.workspaceRoot, root) }).filter(
        (t) => !t.endsWith('.device.test.ts'),
      );
      if (hostTests.length)
        project.targets = {
          'test-host': {
            executor: 'nx:run-commands',
            cache: false,
            inputs: ['hostTests'],
            outputs: [],
            options: {
              command: `pnpm exec vitest run --config vitest.host.config.ts --project module-tests ${root}`,
              cwd: '.',
            },
          },
        };
      return [f, { projects: { [root]: project } }];
    }),
];

export const createDependencies = (_opts, ctx) => {
  const mods = Object.values(ctx.projects).filter((p) => p.tags?.includes('module'));
  const owner = (path) =>
    mods
      .filter((p) => path === p.root || path.startsWith(p.root + '/'))
      .sort((a, b) => b.root.length - a.root.length)[0]?.name;
  return mods.flatMap((p) => {
    const t = read(ctx.workspaceRoot, join(p.root, 'module.toml'));
    const deps = [...(t.module.requires ?? [])];
    for (const o of t.object ?? []) for (const s of o.sources ?? []) deps.push(owner(normalize(join(p.root, s))) ?? '');
    return [...new Set(deps)]
      .filter((d) => d && d !== p.name)
      .map((target) => ({
        source: p.name,
        target,
        sourceFile: join(p.root, 'module.toml'),
        type: DependencyType.static,
      }));
  });
};

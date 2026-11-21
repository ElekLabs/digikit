import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, matchesGlob } from 'node:path';
import { afterAll, beforeAll, expect, it } from 'vitest';
import { createNodesV2 } from '../modules.mjs';
import { packAliasesTarget } from '../module-build.mjs';

const root = mkdtempSync(join(tmpdir(), 'digikit-module-builds-'));
afterAll(() => rmSync(root, { recursive: true, force: true }));

beforeAll(() => {
  mkdirSync(join(root, 'tests/regression'), { recursive: true });
  writeFileSync(
    join(root, 'tests/regression/tsconfig.json'),
    JSON.stringify({ compilerOptions: {}, include: ['../fixtures/**/*.ts'] }),
  );
});

it('infers core producers from category and source-backed objects, independently of IDs and types', () => {
  const modules = [
    {
      dir: 'image-modules/arbitrary/owner',
      id: 'example/owner',
      category: 'core',
      requires: ['example/dep', 'core/content'],
      sources: ['src/a.c'],
    },
    { dir: 'modules/arbitrary/dep', id: 'example/dep', category: 'core', sources: ['src/b.c'] },
    { dir: 'modules/arbitrary/content', id: 'core/content', category: 'test', sources: ['src/c.c'] },
    { dir: 'modules/arbitrary/prebuilt', id: 'example/prebuilt', category: 'core', sources: [] },
    { dir: 'modules/arbitrary/unrelated', id: 'example/unrelated', category: 'core', sources: ['src/d.c'] },
    { dir: 'relocated/fixtures/example', id: 'fixture/example', category: 'test', sources: ['src/f.c'] },
    { dir: 'relocated/tools/example', id: 'tool/example', category: 'tools', sources: ['src/t.c'] },
  ];
  const files = modules.map((m) => {
    const file = `${m.dir}/module.toml`;
    mkdirSync(join(root, dirname(file)), { recursive: true });
    writeFileSync(
      join(root, file),
      `[module]\nid = "${m.id}"\ncategory = "${m.category}"\ntype = "raw"\ndynamic = true\nrequires = ${JSON.stringify(m.requires ?? [])}\n[[object]]\npath = "build/object.dko.json"\nsources = ${JSON.stringify(m.sources)}\n`,
    );
    return file;
  });
  mkdirSync(join(root, 'tools/patchkit/toolchain'), { recursive: true });
  writeFileSync(join(root, 'tools/patchkit/toolchain/pins.toml'), '[include]\ncf = ["sdk/include"]\nsharc = []\n');
  mkdirSync(join(root, 'devices'));
  writeFileSync(join(root, 'devices/example.toml'), '[patch_sets]\nemulator = ["example/owner"]\n');
  const template = 'sdk/templates/example/module.toml';
  mkdirSync(join(root, dirname(template)), { recursive: true });
  writeFileSync(join(root, template), 'placeholder for module:new');
  const matches = [...files, template].filter((file) => matchesGlob(file, createNodesV2[0]));
  expect(matches).toEqual([...files, template]);
  const inferred = createNodesV2[1](matches, {}, { workspaceRoot: root });
  const projects = Object.assign({}, ...inferred.map(([, result]) => result.projects));
  const owner = projects[modules[0].dir];
  expect(owner.tags).toEqual(expect.arrayContaining(['module', 'type:raw', 'acceptance-image']));
  const producer = owner.targets['build-objects'];
  for (const module of modules) {
    const input = `{workspaceRoot}/${module.dir}/module.toml`;
    if (module.category === 'core') expect(producer.inputs).toContain(input);
    else expect(producer.inputs).not.toContain(input);
  }
  expect(producer.inputs).not.toContain('{workspaceRoot}/modules/**/module.toml');
  expect(producer.inputs).not.toContain('{workspaceRoot}/tests/fixtures/modules/**/module.toml');

  expect(producer.outputs).toEqual([`{workspaceRoot}/${modules[0].dir}/build/object.dko.json`]);
  expect(producer.dependsOn).toEqual(['sdk:check-generated', 'patchkit:cli', 'example/dep:build-objects']);
  expect(projects[modules[1].dir].tags).toContain('acceptance-image');
  expect(projects[modules[2].dir].targets?.['build-objects']).toBeUndefined();
  expect(projects[modules[3].dir].targets?.['build-objects']).toBeUndefined();
  expect(projects[modules[4].dir].targets['build-objects']).toBeDefined();
  expect(projects[modules[4].dir].tags).not.toContain('acceptance-image');
  const fixture = projects['relocated/fixtures/example'];
  expect(fixture.tags).not.toContain('release-module');
  expect(fixture.targets?.pack).toBeUndefined();
  expect(fixture.targets?.['test-conformance']).toBeUndefined();
  const tool = projects['relocated/tools/example'];
  expect(tool.tags).toContain('release-module');
  expect(tool.targets.pack).toBeDefined();
  expect(tool.targets['test-conformance']).toBeDefined();
  expect(projects[dirname(template)]).toBeUndefined();
});

it('scopes explicit acceptance images without changing the default image producer set', () => {
  const owners = [{ name: 'scoped-ui-tests', patchModules: ['example/dep'] }, { name: 'default-ui-tests' }];
  const files = owners.map(({ name, patchModules }) => {
    const dir = `relocated/${name}/tests/zero`;
    mkdirSync(join(root, dir), { recursive: true });
    writeFileSync(join(root, dir, 'ui.device.test.ts'), 'export {};');
    writeFileSync(
      join(root, dir, 'project.json'),
      JSON.stringify({
        name,
        tags: ['test-only'],
        ...(patchModules ? {} : { implicitDependencies: ['example/owner'] }),
        metadata: {
          runtimeAcceptance: [
            {
              files: ['ui.device.test.ts'],
              tags: ['C:loading', 'M:tool/example'],
              modules: ['fixture/example'],
              patchModules,
            },
          ],
        },
      }),
    );
    return `${dir}/project.json`;
  });
  const manifests = ['owner', 'dep', 'content', 'prebuilt', 'unrelated'].map(
    (name) => `${name === 'owner' ? 'image-modules' : 'modules'}/arbitrary/${name}/module.toml`,
  );
  manifests.push('relocated/tools/example/module.toml', 'relocated/fixtures/example/module.toml');
  const inferred = createNodesV2[1]([...manifests, ...files], {}, { workspaceRoot: root });
  const projects = Object.assign({}, ...inferred.map(([, result]) => result.projects));
  // the run targets are the device halves; `test-device` is their no-op parent
  const scoped = projects['relocated/scoped-ui-tests/tests/zero'].targets['test-device--dt2'];
  const normal = projects['relocated/default-ui-tests/tests/zero'].targets['test-device--dn2'];
  expect(projects['relocated/fixtures/example'].targets.pack).toBeDefined();
  expect(scoped.dependsOn).toContain('fixture/example:pack');
  expect(scoped.dependsOn).toContain('example/dep:build-objects');
  expect(scoped.dependsOn).not.toContain('example/owner:build-objects');
  expect(scoped.inputs).toContainEqual({ input: 'production', projects: ['example/dep'] });
  expect(normal.dependsOn).toContainEqual({ target: 'build-objects', projects: ['tag:acceptance-image'] });
});

it('refuses distinct aliases that would overwrite the same pack output', () => {
  const source = 'test/machine';
  const byId = new Map([[source, { file: 'fixtures/machine/module.toml', manifest: { id: source } }]]);
  expect(() => packAliasesTarget(root, byId, [{ aliases: { 'first/alias': source, 'second/alias': source } }])).toThrow(
    'Pack aliases first/alias and second/alias both write',
  );
});

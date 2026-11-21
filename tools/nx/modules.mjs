// Nx plugin: one project per module.toml. Edges: [module].requires plus [[object]].sources paths
// that resolve into another module directory. tools/devtools/nx_graph.py reads them from the graph.
// Targets: pack (build stale objects and pack to dist/modules/<dev>/<leaf>.dkm), pack-aliases (renamed copies named by
// metadata.runtimeAcceptance), test-host, test-device and test-conformance (vitest directly, after the packs).
// This file wires the plugin. Build and pack targets are in module-build.mjs, device-test cache keys in
// device-test-keys.mjs, and the shape of device, conformance and test-only targets in device-tests.mjs.
import { globSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import {
  read,
  records,
  mergeTests,
  manifestClosure,
  packDevices,
  isUploadable,
  isFixture,
} from './module-manifests.mjs';
import { buildObjectsTarget, packTarget, packAliasesTarget } from './module-build.mjs';
import { packSourceInputs } from './pack-inputs.mjs';
import { deviceTargets, checkMergeTests, conformanceTarget, testOnlyNodes, regressionNode } from './device-tests.mjs';
import { serviceLeaves, serviceIds } from './device-test-keys.mjs';
import { checkAcceptanceTags } from './acceptance-tags.mjs';
import { standaloneRustTests } from './cargo.mjs';
import {
  allRigConformance,
  fxWriteConformance,
  moduleRigConformance,
  DSP_CONFORMANCE,
  DSP_CONFORMANCE_ALL,
  RIG_KIT_ROOT,
  rigTarget,
} from './rig-tests.mjs';
import { testImportInputs } from './ts-imports.mjs';
import { pythonImportInputs } from './py-imports.mjs';

export { createDependencies } from './module-dependencies.mjs';

export const createNodesV2 = [
  '**/{module.toml,project.json}',
  (matched, _opts, ctx) => {
    // Lazy for manifests without device targets (including small plugin fixtures).
    let imports;
    const importInputs = (files) => (imports ??= testImportInputs(ctx.workspaceRoot))(files);
    let py;
    const pythonInputs = (seeds) => (py ??= pythonImportInputs(ctx.workspaceRoot))(seeds);
    // Nx supplies the workspace file list, with ignored build trees and dependencies already excluded.
    const files = matched.filter((f) => f.endsWith('/module.toml') && !f.startsWith('sdk/templates/'));
    const projectFiles = matched
      .filter((f) => f.endsWith('/project.json'))
      .map((f) => [f, JSON.parse(readFileSync(join(ctx.workspaceRoot, f), 'utf8'))]);
    const testOwners = projectFiles.filter(([, p]) => p.tags?.includes('test-only'));
    // id -> manifest, so an alias can find the devices of its source module.
    const byId = new Map();
    for (const file of files) {
      const doc = read(ctx.workspaceRoot, file);
      const { module: manifest, object: objects = [] } = doc;
      const previous = byId.get(manifest.id);
      if (previous) throw new Error(`Duplicate module ${manifest.id} in ${previous.file} and ${file}`);
      byId.set(manifest.id, { file, manifest, objects, doc });
    }
    const moduleInfo = new Map([...byId].map(([id, { manifest }]) => [id, manifest]));
    const coreManifests = [...byId.values()]
      .filter(({ manifest }) => manifest.category === 'core')
      .map(({ file }) => file)
      .sort();
    const coreBuilds = new Set(
      [...byId]
        .filter(([, { manifest, objects }]) => manifest.category === 'core' && objects.some((o) => o.sources?.length))
        .map(([id]) => id),
    );
    // Patch image composition follows module.requires, not acceptance-fixture graph edges.
    // imageModules: id -> Set of device shorts whose patch-set images hold the module.
    const imageModules = new Map();
    const includeImageModule = (id, device) => {
      const manifest = byId.get(id)?.manifest;
      if (!manifest) throw new Error(`Unknown acceptance patch-set module ${id}`);
      if (imageModules.get(id)?.has(device)) return;
      if (!imageModules.has(id)) imageModules.set(id, new Set());
      imageModules.get(id).add(device);
      for (const required of manifest.requires ?? []) includeImageModule(required, device);
    };
    // The patch sets of the device profiles; arbitrary overrides need separate input review.
    for (const profile of globSync('devices/*.toml', { cwd: ctx.workspaceRoot }).sort()) {
      const doc = read(ctx.workspaceRoot, profile);
      const sets = doc.patch_sets ?? {};
      // every set: tools/nx/device-bytes.mjs hashes each one, so their objects are built first
      for (const ids of Object.values(sets)) for (const id of ids) includeImageModule(id, doc.device?.short ?? profile);
    }
    // Only the modules the gate packs get a pack target: patched-in core modules and static fixtures
    // (several ColdFire objects, no dkm_ symbols) cannot be packed as .dkm.
    const packable = new Set([...byId].filter(([, { manifest }]) => isUploadable(manifest)).map(([id]) => id));
    for (const [file, project] of projectFiles) {
      const recs = project.metadata?.runtimeAcceptance ?? [];
      checkAcceptanceTags(dirname(file), recs, [], moduleInfo);
      for (const rec of recs) {
        for (const id of rec.modules ?? []) packable.add(id);
        for (const source of Object.values(rec.aliases ?? {})) packable.add(source);
      }
    }
    const nodes = [...byId.values()].map(({ file: f, manifest: m, objects, doc }) => {
      const root = dirname(f);
      const project = {
        name: m.id,
        root,
        tags: ['module', `type:${m.type}`],
      };
      if (!isFixture(m) && m.category !== 'core') project.tags.push('release-module');
      if (coreBuilds.has(m.id) && imageModules.has(m.id)) project.tags.push('acceptance-image');
      const tests = globSync('tests/**/*.test.ts', { cwd: join(ctx.workspaceRoot, root) }).filter(
        (t) =>
          !t.endsWith('.manual.device.test.ts') &&
          !testOwners.some(([pf]) => join(root, t).startsWith(dirname(pf) + '/')),
      );
      const hostTests = tests.filter((t) => !t.endsWith('.device.test.ts') && !t.endsWith('.rig.test.ts'));
      const rigTests = tests.filter((t) => t.endsWith('.rig.test.ts'));
      const deviceTests = tests.filter((t) => t.endsWith('.device.test.ts')).sort();
      const shipped = !isFixture(m);
      const targets = {};
      const uploadable = isUploadable(m);
      const recs = records(join(ctx.workspaceRoot, root));
      const devices = packDevices(m);
      if (coreBuilds.has(m.id))
        targets['build-objects'] = buildObjectsTarget(
          ctx.workspaceRoot,
          byId,
          m,
          root,
          objects,
          coreBuilds,
          coreManifests,
        );
      if (devices.length && packable.has(m.id)) targets.pack = packTarget(ctx.workspaceRoot, byId, m, devices);
      const aliasesTarget = packAliasesTarget(ctx.workspaceRoot, byId, recs);
      if (aliasesTarget) targets['pack-aliases'] = aliasesTarget;
      if (shipped && hostTests.length)
        targets['test-host'] = {
          executor: 'nx:run-commands',
          cache: false,
          inputs: ['hostTests'],
          outputs: [],
          options: {
            command: `pnpm exec vitest run --config vitest.host.config.ts --project module-tests ${root}/`,
            cwd: '.',
          },
        };
      checkMergeTests(ctx.workspaceRoot, root, deviceTests, false);
      checkAcceptanceTags(root, recs, deviceTests, moduleInfo);
      if (shipped && deviceTests.length)
        Object.assign(
          targets,
          deviceTargets('test-device', root, deviceTests, recs, null, importInputs, pythonInputs, uploadable, {
            twin: true,
            moduleId: m.id,
            services: serviceLeaves(byId),
            serviceIds: serviceIds(byId),
            projectDevices: devices,
            mergeTests: mergeTests(join(ctx.workspaceRoot, root)),
            base: ctx.workspaceRoot,
          }),
        );
      // standalone Rust host tests of the module crate
      const rustTests = standaloneRustTests(ctx.workspaceRoot, root);
      if (rustTests) targets['rust-tests'] = rustTests;
      // sound checks on the SHARC-only rig (packages/sharc-rig-kit, docs/TESTING.md "Sound checks"): tests/rig/*.toml run by tests/*.rig.test.ts
      const rig = moduleRigConformance(ctx.workspaceRoot, f, doc);
      if (rigTests.length) targets['test-dsp'] = rigTarget(root, rig?.soundInputs);
      if (rig) targets[DSP_CONFORMANCE] = rig.target;
      if (uploadable) {
        targets['test-conformance-all'] = conformanceTarget(ctx.workspaceRoot, root, m, f, importInputs, true);
        // core/runtime-fixture-tests exercises the shared loader for machines and effects. A change to either
        // module's private pack inputs schedules its own emulator conformance task.
        targets['test-conformance'] =
          (m.type === 'machine' || m.type === 'fx') && m.dynamic === true && m.category !== 'core'
            ? {
                executor: 'nx:noop',
                cache: false,
                inputs: packSourceInputs(ctx.workspaceRoot, byId, manifestClosure(byId, [m.id]), { privateOnly: true }),
                dependsOn: [{ target: 'test-conformance-all', params: 'forward' }],
              }
            : conformanceTarget(ctx.workspaceRoot, root, m, f, importInputs);
      }
      if (Object.keys(targets).length) project.targets = targets;
      return [f, { projects: { [root]: project } }];
    });
    const rigProjectFile = `${RIG_KIT_ROOT}/project.json`;
    if (matched.includes(rigProjectFile))
      nodes.push([
        rigProjectFile,
        {
          projects: {
            [RIG_KIT_ROOT]: {
              targets: {
                [DSP_CONFORMANCE_ALL]: allRigConformance(),
                [DSP_CONFORMANCE]: fxWriteConformance(),
              },
            },
          },
        },
      ]);
    testOnlyNodes(ctx.workspaceRoot, testOwners, byId, moduleInfo, coreBuilds, importInputs, pythonInputs, nodes, imageModules);
    if (matched.includes('tests/regression/project.json')) nodes.push(regressionNode(ctx.workspaceRoot, importInputs));
    return nodes;
  },
];

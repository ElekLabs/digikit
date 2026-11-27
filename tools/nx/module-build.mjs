// Build and pack: the patchkit command, the build input lists and the build-objects, pack and pack-aliases targets.
// It does not hold device-test cache keys (device-test-keys.mjs) or the shape of test targets (device-tests.mjs).
import { packOut, packMap, servicesOut, manifestClosure, manifestArgs, packDevices } from './module-manifests.mjs';
import { packSourceInputs } from './pack-inputs.mjs';

// Commands run with cwd '.' (the workspace root); Nx accepts {workspaceRoot} only at the start of an option.
export const PK = 'dist/patchkit/patchkit --root .';
// Shared compiler/contract surfaces. Optional implementation sources are named by their consumers.
export const SHARED_SDK_INPUTS = [
  '{workspaceRoot}/sdk/sdk.toml',
  '{workspaceRoot}/sdk/include/**/*',
  '{workspaceRoot}/sdk/dsp/contract/**/*',
  '{workspaceRoot}/sdk/dsp/**/*',
  '{workspaceRoot}/runtime/core/vectors/**/*',
];
export const ALL_MANIFESTS = new Set([
  '{workspaceRoot}/{modules,runtime,services}/**/module.toml',
  '{workspaceRoot}/tests/fixtures/modules/**/module.toml',
]);
// Build/pack producers do not read acceptance tests. Nx's implicit ^default input would
// otherwise turn a harness-only edit into a changed image output for every consumer.
// Plugin sources are not inputs: Nx hashes each project's resolved configuration, so a plugin edit reaches only targets whose definition changes.
// A pack or object build reads the SDK headers its sources include, not every header: packSourceInputs lists those.
const SDK_INCLUDE_GLOB = '{workspaceRoot}/sdk/include/**/*';
export const BUILD_INPUTS = [
  'production',
  '^production',
  'jsToolchain',
  { dependentTasksOutputFiles: '**/*', transitive: true },
  // gen reads the library manifests and device facts; builders read shared SDK headers and pins.
  ...ALL_MANIFESTS,
  // pack and build-objects run the Rust patchkit binary (its build output arrives through dependentTasksOutputFiles, so the workspace
  // Cargo.toml and Cargo.lock are no inputs: a Rust object is staged from its own crate and path dependencies, packSourceInputs), which reads devices/*.toml and the toolchain pins;
  // pk_toolchains.py pins the m68k compiler; there is no Python on this path.
  '{workspaceRoot}/devices/*.toml',
  ...SHARED_SDK_INPUTS,
  '{workspaceRoot}/tools/patchkit/toolchain/**/*',
  '{workspaceRoot}/tools/patchkit/pk_toolchains.py',
  '{workspaceRoot}/.cargo/**/*',
  '{workspaceRoot}/rust-toolchain*',
  '{workspaceRoot}/mise.toml',
];

// These commands receive an explicit library; shared devices, SDK and tool inputs stay conservative.
export const scopedManifestInputs = (inputs, manifests) => [
  ...inputs.filter((input) => !ALL_MANIFESTS.has(input)),
  ...manifests.map((file) => `{workspaceRoot}/${file}`),
];

// Core generation reads the allocation providers and declarations of the complete core library.
// Build inputs follow manifest/source ownership, never the module's acceptance relationships.
export const packInputs = (workspaceRoot, byId, manifests) => [
  ...scopedManifestInputs(
    BUILD_INPUTS.filter((input) => input !== '^production' && input !== SDK_INCLUDE_GLOB),
    manifests,
  ),
  ...packSourceInputs(workspaceRoot, byId, manifests),
];

// Like pack, build-objects reads the sources of the module's manifest closure (packSourceInputs) and the patchkit binary it
// runs, not the production files of every graph dependency (`^production` reached the test harness and its packages).
export const buildObjectsTarget = (workspaceRoot, byId, m, root, objects, coreBuilds, coreManifests) => ({
  executor: 'nx:run-commands',
  cache: false,
  inputs: [
    ...scopedManifestInputs(
      BUILD_INPUTS.filter((input) => input !== '^production' && input !== SDK_INCLUDE_GLOB),
      coreManifests,
    ),
    ...packSourceInputs(workspaceRoot, byId, manifestClosure(byId, [m.id])),
    // The build is not cached: a patchkit source edit must still run it (a changed generator changes the objects).
    { input: 'production', projects: ['patchkit'] },
  ],
  description: 'Check generated sources, then incrementally build committed core objects; may update tracked outputs.',
  dependsOn: [
    'sdk:check-generated',
    'patchkit:cli',
    ...(m.requires ?? []).filter((id) => coreBuilds.has(id)).map((id) => `${id}:build-objects`),
  ],
  // `patchkit build` also rewrites the system package object ([system_package]) and the object the package links in
  // ([package_part], the DIGIKIT menu): a task that reads one names it through this target.
  outputs: [...objects, ...[byId.get(m.id)?.doc.system_package, byId.get(m.id)?.doc.package_part].filter(Boolean)]
    .filter((o) => o.sources?.length)
    .map((o) => `{workspaceRoot}/${root}/${o.path}`),
  options: {
    commands: [
      `${PK} gen --modules ${m.id} ${manifestArgs(coreManifests)} --check --scope modules`,
      `${PK} build ${root}`,
    ],
    parallel: false,
    cwd: '.',
  },
});

// `patchkit services` maps the NEEDS of a packed file to the service modules of all manifests: their manifests are inputs of the pack target besides its closure.
const serviceManifests = (byId) =>
  [...byId.values()].filter(({ manifest }) => manifest.role === 'service').map(({ file }) => file);

export const packTarget = (workspaceRoot, byId, m, devices) => {
  const manifests = manifestClosure(byId, [m.id]);
  return {
    executor: 'nx:run-commands',
    cache: false,
    inputs: [
      ...packInputs(workspaceRoot, byId, manifests),
      ...serviceManifests(byId).map((file) => `{workspaceRoot}/${file}`),
    ],
    dependsOn: ['patchkit:cli'],
    outputs: devices.flatMap((d) => [
      `{workspaceRoot}/${packOut(d, m)}`,
      `{workspaceRoot}/${packMap(d, m)}`,
      `{workspaceRoot}/${servicesOut(d, m)}`,
    ]),
    options: {
      commands: [
        'mkdir -p dist/modules/dt2 dist/modules/dn2',
        ...devices.map(
          (d) =>
            `${PK} pack ${m.id} ${manifestArgs(manifests)} --device ${d} --addrmap ${packMap(d, m)} -o ${packOut(d, m)} && ${PK} services --device ${d} ${packOut(d, m)} > ${servicesOut(d, m)}`,
        ),
      ],
      parallel: false,
      cwd: '.',
    },
  };
};

// A module with a `workbench.toml` keeps a generated `workbench/gen/settings.ts`. The check fails when it is missing or differs from what
// `patchkit workbench-ts` writes. It reads the two files, the generated file and the patchkit binary (the output of `patchkit:cli`).
export const workbenchTsCheckTarget = (root) => ({
  executor: 'nx:run-commands',
  cache: true,
  inputs: [
    `{workspaceRoot}/${root}/workbench.toml`,
    `{workspaceRoot}/${root}/module.toml`,
    `{workspaceRoot}/${root}/workbench/gen/settings.ts`,
    { dependentTasksOutputFiles: '**/*', transitive: true },
  ],
  dependsOn: ['patchkit:cli'],
  outputs: [],
  options: { command: `${PK} workbench-ts ${root} --check`, cwd: '.' },
});

// Returns undefined when no record names an alias with a known source module.
export const packAliasesTarget = (workspaceRoot, byId, recs) => {
  const aliasCmds = [];
  const aliasOutputs = [];
  const aliasSources = new Set();
  const sourceOfAlias = new Map();
  const aliasOfOutput = new Map();
  for (const rec of recs)
    for (const [alias, source] of Object.entries(rec.aliases ?? {})) {
      // A source outside the scanned module.toml files has no manifest: its devices are unknown, skip it.
      const src = byId.get(source);
      if (!src) continue;
      const previous = sourceOfAlias.get(alias);
      if (previous && previous !== source) throw new Error(`Pack alias ${alias} names both ${previous} and ${source}`);
      sourceOfAlias.set(alias, source);
      aliasSources.add(source);
      for (const d of packDevices(src.manifest)) {
        const output = packOut(d, src.manifest, alias);
        const existing = aliasOfOutput.get(output);
        if (existing && existing !== alias)
          throw new Error(`Pack aliases ${existing} and ${alias} both write ${output}`);
        aliasOfOutput.set(output, alias);
        aliasCmds.push(
          `${PK} pack ${source} ${manifestArgs(manifestClosure(byId, [source]))} --device ${d} --name ${alias} -o ${output} && ${PK} services --device ${d} ${output} > ${servicesOut(d, src.manifest, alias)}`,
        );
        aliasOutputs.push(`{workspaceRoot}/${output}`, `{workspaceRoot}/${servicesOut(d, src.manifest, alias)}`);
      }
    }
  if (!aliasCmds.length) return undefined;
  return {
    executor: 'nx:run-commands',
    cache: false,
    inputs: packInputs(workspaceRoot, byId, manifestClosure(byId, [...aliasSources])),
    dependsOn: ['patchkit:cli', ...[...aliasSources].map((s) => `${s}:pack`)],
    outputs: aliasOutputs,
    options: {
      commands: ['mkdir -p dist/modules/dt2 dist/modules/dn2', ...aliasCmds],
      parallel: false,
      cwd: '.',
    },
  };
};

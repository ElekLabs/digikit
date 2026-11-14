// Module build and acceptance target inputs and factories.
import { existsSync } from 'node:fs';
import { basename, join } from 'node:path';

// Commands run with cwd '.' (the workspace root); Nx accepts {workspaceRoot} only at the start of an option.
export const PK = 'dist/patchkit/patchkit --root .';
export const IMAGE_BUILD = { target: 'build-objects', projects: ['tag:acceptance-image'] };
// Writes the hash of every committed core object without its source hash (tools/nx/device-bytes.mjs).
export const DEVICE_BYTES = 'patchkit:device-bytes';
// Writes the hash of the image, plan and address map that `patchkit apply` makes for each acceptance firmware and patch set
// (tools/nx/device-images.mjs).
export const DEVICE_IMAGES = 'patchkit:device-images';
// Writes the `needs` verdict of the packs in tests/regression/runtime/needs-packs.json (tools/nx/pack-verdicts.mjs).
export const PACK_VERDICTS = 'patchkit:pack-verdicts';
const PACK_NEEDS = '{workspaceRoot}/tests/regression/runtime/pack-needs.ts';
// Shared compiler/contract surfaces. Optional implementation sources are named by their consumers.
export const SHARED_SDK_INPUTS = [
  '{workspaceRoot}/sdk/sdk.toml',
  '{workspaceRoot}/sdk/include/**/*',
  '{workspaceRoot}/sdk/dsp/contract/**/*',
  '{workspaceRoot}/sdk/dsp/**/*',
  '{workspaceRoot}/modules/core/runtime/vectors/**/*',
];
// Build/pack producers do not read acceptance tests. Nx's implicit ^default input would
// otherwise turn a harness-only edit into a changed image output for every consumer.
// Plugin sources are not inputs: Nx hashes each project's resolved configuration, so a plugin edit reaches only targets whose definition changes.
export const BUILD_INPUTS = [
  'production',
  '^production',
  'jsToolchain',
  { dependentTasksOutputFiles: '**/*', transitive: true },
  // gen reads the library manifests and device facts; builders read shared SDK headers and pins.
  '{workspaceRoot}/modules/**/module.toml',
  '{workspaceRoot}/tests/fixtures/modules/**/module.toml',
  // pack and build-objects run the Rust patchkit binary, which reads devices/*.toml and the toolchain pins;
  // pk_toolchains.py pins the m68k compiler; there is no Python on this path.
  '{workspaceRoot}/devices/*.toml',
  ...SHARED_SDK_INPUTS,
  '{workspaceRoot}/tools/patchkit/toolchain/**/*',
  '{workspaceRoot}/tools/patchkit/pk_toolchains.py',
  '{workspaceRoot}/Cargo.toml',
  '{workspaceRoot}/Cargo.lock',
  '{workspaceRoot}/.cargo/**/*',
  '{workspaceRoot}/rust-toolchain*',
  '{workspaceRoot}/mise.toml',
];

// These lists hold the module-level import closure of the tools device tests spawn, plus the files in
// tools/patchkit and tools/device that those tools import from functions. Other tools/coldfire and
// tools/sharc_dev files are imported only from functions and are left out.
// Extend a list when a test spawns a new tool.
export const PYTHON_CLIENT_SOURCES = [
  'tools/__init__.py',
  'tools/control/digi_client.py',
  'tools/devtools/__init__.py',
  'tools/devtools/portable_flock.py',
  'tools/devtools/stablebin.py',
  'tools/emulator/__init__.py',
  'tools/emulator/digi_env.py',
  'tools/emulator/native_target.py',
  'tools/firmware/*.py',
  'tools/patchkit/__init__.py',
  'tools/patchkit/dsp_patch.py',
  'tools/patchkit/modroots.py',
  'tools/patchkit/patchkit.py',
].map((path) => `{workspaceRoot}/${path}`);

export const PYTHON_TOOL_SOURCES = [
  ...PYTHON_CLIENT_SOURCES,
  ...[
    'tools/coldfire/__init__.py',
    'tools/coldfire/arena_safety.py',
    'tools/coldfire/panel_script.py',
    'tools/coldfire/regions.py',
    'tools/control/__init__.py',
    'tools/device/*.py',
    'tools/devtools/benchlock.py',
    'tools/devtools/render_cache.py',
    'tools/emulator/card_check.py',
    'tools/emulator/card_overlay.py',
    'tools/emulator/coupled.py',
    'tools/emulator/dt2_sample_check.py',
    'tools/emulator/native_pgo.py',
    'tools/emulator/snapread.py',
    'tools/patchkit/hook_ab.py',
    'tools/patchkit/pk_device.py',
    'tools/patchkit/pk_digikit_*.py',
    'tools/patchkit/pk_dkrt*.py',
    'tools/patchkit/pk_dsp_budget.py',
    'tools/patchkit/pk_emud_pgo.py',
    'tools/patchkit/pk_harness.py',
    'tools/patchkit/pk_mem.py',
  ].map((path) => `{workspaceRoot}/${path}`),
];

// These commands receive an explicit library; shared devices, SDK and tool inputs stay conservative.
const ALL_MANIFESTS = new Set([
  '{workspaceRoot}/modules/**/module.toml',
  '{workspaceRoot}/tests/fixtures/modules/**/module.toml',
]);
export const scopedManifestInputs = (inputs, manifests) => [
  ...inputs.filter((input) => !ALL_MANIFESTS.has(input)),
  ...manifests.map((file) => `{workspaceRoot}/${file}`),
];

// Files that tests read by path and do not import. The import walk (test-imports.mjs) cannot see them.
// Extend this list when a test reads a new file; a file only one record's tests read goes in that record's `sources`.
// SHARED_READS are files that every device test may read and the built bytes do not carry.
const workspaceFile = (file) => `{workspaceRoot}/${file}`;
const SHARED_READS = [
  // tests/regression/runtime/device-data.ts and card-cache.ts read the profiles and the firmware ports
  { file: 'devices/**/*.toml' },
  // the manifests of the core modules: `patchkit apply` reads them with the objects (the image, plan and address map in
  // dist/bytes/images.sha256 carry the effect); only the memaudit helper reads them itself
  { file: 'modules/core/**/module.toml', readers: ['tests/regression/runtime/memaudit-kit.ts'] },
  // upload-kit.ts reads the runtime version and the `since` of the imports
  { file: 'sdk/sdk.toml', readers: ['tests/regression/runtime/upload-kit.ts'] },
  // synth.ts reads the machine types
  { file: 'modules/core/adapters/machines/types.toml', readers: ['tests/regression/runtime/synth.ts'] },
];
const TEST_READS = [
  // fixture files
  '{workspaceRoot}/modules/core/runtime/tests/fixtures/safe-*.bin',
  '{workspaceRoot}/tests/fixtures/sharc/**/*.json',
  // the Vitest run itself
  '{workspaceRoot}/tests/regression/vitest.config.ts',
  '{workspaceRoot}/tests/regression/tsconfig.json',
  '{workspaceRoot}/tests/regression/package.json',
  // read at run time by runtime-plan.ts and pack-needs.ts
  '{workspaceRoot}/tests/regression/runtime/image-roots.json',
  '{workspaceRoot}/tests/regression/runtime/needs-packs.json',
  '{workspaceRoot}/tools/emulator/ready_inputs.json',
];
// Full keys list every shared file. A merge key lists a file only when the test (its import closure) reads it, or runs patchkit
// live: the other effects of the file are in the bytes the key hashes.
const sharedReads = (imports = null, patchkitLive = false) =>
  SHARED_READS.filter(
    ({ readers }) =>
      !imports || !readers || patchkitLive || readers.some((reader) => imports.includes(workspaceFile(reader))),
  ).map(({ file }) => workspaceFile(file));
export const READ_BY_TESTS = [...sharedReads(), ...TEST_READS];

// Core-change policy. A change runs all device tests of the projects it edits. A device test that a change reaches
// only through shared inputs (other core modules, SDK, device profiles, patchkit, the daemon) runs only if it is
// listed in `metadata.mergeTests` of its project.json (paths relative to the project root). Every other file gets a
// narrow merge key (its own folder, test code and loaded packs), so Nx replays its last result. `mise run test:full`
// runs every file with full inputs (`test-device-all`); run it before each push or PR to main.
// Vitest runs this file before each test file (`setupFiles`), so no test imports it.
export const VITEST_SETUP = 'tests/regression/setup-temp.ts';

// Cache inputs of test-device, test-device-all and test-conformance. Two sets exist:
// - full (DEVICE_INPUTS plus the image input and every pack output): test-conformance, `test-device-all`, and the
//   merge target of a file in `metadata.mergeTests`;
// - narrow (narrowDeviceInputs): the merge target of any other file. It drops the SDK, device profiles, committed
//   core objects, the image input and the blanket pack outputs, and keeps the packs the test loads (and, for a
//   test-only project, `production` of its subject modules).
// A key covers what the run consumes:
// - the imports of the test files and of the Vitest setup file (importInputs, a closure over TypeScript sources);
// - the outputs of the pack, CLI and daemons targets (dependentTasksOutputFiles; dist/daemons.json names the
//   content-keyed daemon), and the production files of the core modules of the patched image (imageInput);
// - the module itself (`production` of the project: its sources, without tests/ and docs);
// - the files in READ_BY_TESTS, the SDK, config/emulator.json (sets SHARC_* env) and each record's `sources`;
// - the firmware and ready dirs via device-inputs.mjs, and the env switches below.
// A package or a harness file that no test imports is not an input.
// Inputs of both sets.
const DEVICE_COMMON = [
  'jsToolchain',
  { runtime: 'node tools/nx/device-inputs.mjs' },
  { env: 'DIGI_EMU_FAST' },
  { env: 'DIGI_NO_PROGRESS_MS' },
  // quick or full tier (tests/regression/runtime/devices.ts FULL_TIER): selects which tests run.
  { env: 'DIGIKIT_TIER' },
  { env: 'DIGI_PK_READY_DT2_117' },
  { env: 'DIGI_PK_READY_DN2_112' },
  { env: 'SHARC_FAST_REGIONS' },
  { env: 'SHARC_BLOCKTRANS' },
  { env: 'SHARC_FAST_FALLBACK_REGIONS' },
  { env: 'SHARC_FAST_AUTO' },
  { env: 'UPDATE_GOLDENS' },
  '{workspaceRoot}/config/emulator.json',
];
// Full set: what the run consumes is the built bytes (the image hashes, the packs, the tool binaries), not the sources
// they are made from. A source edit that leaves these bytes alone changes no key.
const outputFiles = (globs) => globs.map((glob) => ({ dependentTasksOutputFiles: glob, transitive: true }));
const SHARED_BUILT = [
  'dist/bytes/core-objects.sha256',
  'dist/modules/*/*.dkm',
  'dist/plusdrive/plusdrive',
  'apps/web/public/dksample.wasm',
];
// The binaries that move with every edit of the patchkit sources, whatever bytes the edit packs: the patchkit CLI, and the daemon
// (dist/daemons.json holds the path of the store entry, which is keyed on the patchkit sources too).
const PATCHKIT_BINARIES = ['dist/patchkit/patchkit', 'dist/daemons.json'];
// What a test result follows when it does not run patchkit itself: the patched images, plans and address maps, and the daemon key
// without the sources that only patchkit reads (`emud_key.test_key_of`, written by `verify_build daemons`).
const MERGE_BYTES = ['dist/bytes/images.sha256', 'dist/bytes/daemon-test.key'];
export const BUILT_OUTPUTS = outputFiles([...SHARED_BUILT, ...PATCHKIT_BINARIES]);
// Merge keys: the same without the binaries; a test that runs patchkit live keeps `dist/patchkit/patchkit` (PATCHKIT_LIVE).
export const MERGE_OUTPUTS = outputFiles([...SHARED_BUILT, ...MERGE_BYTES]);
// A test whose import closure holds one of these runs patchkit (every module that executes it imports patchkit.ts, or is listed):
// its key keeps the binary. Only runtime-plan.ts does not count: it runs `apply --set`, and its image, plan and address map are
// hashed in dist/bytes/images.sha256. (An explicit module list goes through runtime-plan-modules.ts, which imports patchkit.ts.)
export const PATCHKIT_LIVE = ['tests/regression/runtime/patchkit.ts'].map((file) => `{workspaceRoot}/${file}`);
export const DEVICE_INPUTS = [...READ_BY_TESTS, ...BUILT_OUTPUTS, ...DEVICE_COMMON];
const mergeDeviceInputs = (imports, patchkitLive) => [
  ...sharedReads(imports, patchkitLive),
  ...TEST_READS,
  ...MERGE_OUTPUTS,
  ...(patchkitLive ? outputFiles(['dist/patchkit/patchkit']) : []),
  ...DEVICE_COMMON,
];
// Tests that run `patchkit build` or `pack` at run time, or patch with explicit fixture modules, compile sources:
// they keep the SDK and the toolchain as inputs (record flag `compiles`, `patchModules`, or any `pythonTools`: the Python devtools may build or pack).
export const COMPILE_INPUTS = [...SHARED_SDK_INPUTS, '{workspaceRoot}/tools/patchkit/toolchain/**/*'];
// Device and conformance tasks run behind this wrapper: it takes one machine-wide emulator slot. It only schedules and is
// no input: it cannot change a result, and Nx does not cache failures.
export const EMU_SLOT = 'node tools/devtools/emu-slot.mjs --';
// Directories of a module project that its objects are built from (see the pack and image outputs).
export const builtSourceExclusions = (root, base = '.') =>
  ['src', 'include', 'fit', 'sdk']
    .filter((dir) => existsSync(join(base, root, dir)) && !root.includes('/tests'))
    .map((dir) => `!{projectRoot}/${dir}/**/*`);
// `opts`: `narrow` selects the narrow input set; `moduleId` is the project's module id (its pack is loaded when
// `uploadable`); `subjects` are the projects a test-only project tests.
export const deviceTarget = (root, tests, recs, imageInput, importInputs, uploadable = false, opts = {}) => {
  const dependsOn = ['patchkit:cli', 'digi-control:daemons', IMAGE_BUILD, DEVICE_BYTES, DEVICE_IMAGES, PACK_VERDICTS];
  if (uploadable) dependsOn.push('pack');
  for (const rec of recs) {
    for (const id of rec.modules ?? []) dependsOn.push(`${id}:pack`);
    if (Object.keys(rec.aliases ?? {}).length) dependsOn.push('pack-aliases');
    if (rec.plusdrive) dependsOn.push('plusdrive-format:cli');
    if (rec.dksampleWasm) dependsOn.push('dksample:wasm');
  }
  // The test files and the setup file with everything they import.
  const imports = importInputs([...tests.map((test) => join(root, test)), VITEST_SETUP]);
  // A test that runs patchkit for its own sake (or patches with explicit fixture modules) depends on the patchkit code, not only on
  // the image bytes: its key keeps the binary.
  const patchkitCompiles = recs.some((rec) => rec.patchModules?.length);
  const patchkitLive = imports.some((file) => PATCHKIT_LIVE.includes(file)) || patchkitCompiles;
  return {
    executor: 'nx:run-commands',
    cache: true,
    inputs: [
      // a test that reads the precomputed `needs` of committed packs keys on that file's content
      ...(imports.includes(PACK_NEEDS) ? outputFiles(['dist/bytes/pack-verdicts.json']) : []),
      ...(opts.narrow && patchkitLive ? outputFiles(['dist/patchkit/patchkit']) : []),
      ...(opts.narrow
        ? [
            ...DEVICE_COMMON,
            ...TEST_READS,
            // the packs the test loads, not every pack output
            // (the own module, the records' modules and their alias packs)
            ...[
              ...new Set(
                [
                  ...(uploadable && opts.moduleId ? [opts.moduleId] : []),
                  ...recs.flatMap((r) => [...(r.modules ?? []), ...Object.keys(r.aliases ?? {})]),
                ].map((id) => id.split('/').at(-1)),
              ),
            ].map((leaf) => ({
              dependentTasksOutputFiles: `dist/modules/*/${leaf}.dkm`,
              transitive: true,
            })),
            // tools the test runs directly
            ...(recs.some((r) => r.plusdrive)
              ? [{ dependentTasksOutputFiles: 'dist/plusdrive/plusdrive', transitive: true }]
              : []),
            ...(recs.some((r) => r.dksampleWasm)
              ? [{ dependentTasksOutputFiles: 'apps/web/public/dksample.wasm', transitive: true }]
              : []),
            // an edit of the subject module reruns a test-only project
            ...(opts.subjects?.length ? [{ input: 'production', projects: opts.subjects }] : []),
            // fixture image modules that a record names are chosen by the test itself
            ...(recs.some((r) => r.patchModules?.length)
              ? [{ input: 'production', projects: [...new Set(recs.flatMap((r) => r.patchModules ?? []))] }]
              : []),
          ]
        : [
            // merge key: the image bytes and the daemon key instead of the binaries
            ...(opts.merge ? mergeDeviceInputs(imports, patchkitLive) : DEVICE_INPUTS),
            // an explicit fixture image is built from the sources of its modules at run time
            ...(imageInput ? [imageInput] : []),
          ]),
      // the module itself (its sources, without tests/ and docs). Full set: the sources that become objects reach the
      // run through the pack outputs and the image hashes.
      ...(opts.narrow ? ['production'] : ['production', ...builtSourceExclusions(root, opts.base)]),
      ...imports,
      ...(!opts.narrow && (patchkitCompiles || recs.some((rec) => rec.compiles || rec.pythonTools))
        ? COMPILE_INPUTS
        : []),
      ...new Set(recs.flatMap((rec) => (rec.sources ?? []).map((s) => `{workspaceRoot}/${s}`))),
      ...(recs.some((rec) => rec.pythonTools === true)
        ? PYTHON_TOOL_SOURCES
        : recs.some((rec) => rec.pythonTools === 'client')
          ? PYTHON_CLIENT_SOURCES
          : []),
    ],
    dependsOn: [...new Set(dependsOn)],
    outputs: [],
    options: {
      command: `${EMU_SLOT} pnpm --filter @digi/regression test:runtime ${tests.map((t) => `../../${root}/${t}`).join(' ')}`,
      cwd: '.',
      // the key holds the patchkit binary (a full key, or a live closure): runtime/patchkit-exec.ts runs patchkit only then
      ...((!opts.narrow && !opts.merge) || patchkitLive ? { env: { DIGI_PATCHKIT_LIVE: '1' } } : {}),
    },
  };
};

// One cached target per test file, so an edit or a failure in one file reruns only that file.
// `name` stays the target everything selects: a no-op parent of the per-file targets.
// With `opts.twin`, `name` is the merge target (narrow keys for files outside `opts.mergeTests`) and `${name}-all`
// runs every file with full inputs.
// Each run target is split in two halves, `<name>--dt2` and `<name>--dn2` (env DIGI_DEVICE), so they run in
// parallel. `<name>` stays as a no-op parent with the inputs of the run target.
export const deviceTargets = (name, root, tests, recs, imageInput, importInputs, uploadable = false, opts = {}) => {
  const targets = fileTargets(name, root, tests, recs, imageInput, importInputs, uploadable, opts);
  const split = {};
  for (const [key, target] of Object.entries(targets)) {
    if (!target.options?.command) {
      split[key] = target;
      continue;
    }
    const halves = ['dt2', 'dn2'].map((device) => `${key}--${device}`);
    split[key] = { executor: 'nx:noop', inputs: target.inputs, dependsOn: halves };
    halves.forEach((half, i) => {
      split[half] = {
        ...target,
        inputs: [...target.inputs, { env: 'DIGI_DEVICE' }],
        options: { ...target.options, env: { ...target.options.env, DIGI_DEVICE: ['dt2', 'dn2'][i] } },
      };
    });
  }
  return split;
};

const fileTargets = (name, root, tests, recs, imageInput, importInputs, uploadable = false, opts = {}) => {
  const allName = `${name}-all`;
  const merge = opts.mergeTests ?? [];
  const isMerge = (test) => !opts.twin || merge.includes(test);
  // `merge`: the key of the merge target (the target everything selects); the `-all` twins keep the full key.
  const build = (list, narrow, recList = recs, merge = false) =>
    deviceTarget(root, list, recList, imageInput, importInputs, uploadable, { ...opts, narrow, merge });
  if (tests.length === 1) {
    const [test] = tests;
    const targets = { [name]: build(tests, !isMerge(test), recs, Boolean(opts.twin)) };
    if (opts.twin) targets[allName] = build(tests, false); // the full key for every test, a merge test included
    return targets;
  }
  const targets = {};
  const all = {};
  for (const test of tests) {
    const suffix = basename(test, '.device.test.ts');
    const child = `${name}--${suffix}`;
    if (targets[child]) throw new Error(`Two device tests of ${root} map to ${child}`);
    const own = recs.filter((rec) => rec.files?.includes(test));
    const narrow = !isMerge(test);
    // A sibling test file is not an input. This file's own closure comes from importInputs.
    const make = (isNarrow, merge = false) => {
      const target = build([test], isNarrow, own.length ? own : recs, merge);
      target.inputs.push('!{projectRoot}/**/*.device.test.ts');
      return target;
    };
    targets[child] = make(narrow, Boolean(opts.twin));
    if (opts.twin) all[`${allName}--${suffix}`] = make(false);
  }
  // Nx selects a target by its inputs, so the parents keep the inputs of the whole project.
  const { inputs } = build(tests, false);
  const children = Object.keys(targets);
  if (opts.twin) {
    for (const [key, value] of Object.entries(all)) if (typeof value !== 'string') targets[key] = value;
    targets[allName] = { executor: 'nx:noop', inputs, dependsOn: Object.keys(all) };
  }
  targets[name] = { executor: 'nx:noop', inputs, dependsOn: children };
  return targets;
};

// What a device test's cache key contains. It does not hold the shape of the targets (device-tests.mjs).
// Three keys exist:
// - full key (DEVICE_INPUTS): all shared reads plus the built outputs, including the patchkit binary;
// - merge key: only the bytes the test loads (the image hashes and the daemon key of its device, its packs) plus the reads its
//   import closure needs, without the files of the other devices; used for the files not listed in metadata.mergeTests;
// - narrow key: the production files of the project, the packs the test loads and its imports.
// Python tool inputs: the import closure (py-imports.mjs) of the `.py` tools a record lists in its `sources`. A test that
// runs a new Python tool must list it there.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { nonCellGlobs, otherFiles } from './device-files.mjs';
import { SHARED_SDK_INPUTS } from './module-build.mjs';

export const PACK_NEEDS = '{workspaceRoot}/tests/regression/runtime/pack-needs.ts';

// Files that tests read by path and do not import. The import walk (ts-imports.mjs) cannot see them.
// Extend this list when a test reads a new file; a file only one record's tests read goes in that record's `sources`.
// SHARED_READS are files that every device test may read and the built bytes do not carry.
export const workspaceFile = (file) => `{workspaceRoot}/${file}`;
const SHARED_READS = [
  // tests/regression/runtime/device-data.ts and card-cache.ts read the profiles and the firmware ports
  { file: 'devices/**/*.toml' },
  // the manifests of the core modules: `patchkit apply` reads them with the objects (the image, plan and address map in
  // dist/bytes/images-<device>.sha256 carry the effect); only the memaudit helper reads them itself
  { file: 'modules/core/**/module.toml', readers: ['tests/regression/runtime/memaudit-kit.ts'] },
  // upload-kit.ts reads the runtime version and the `since` of the imports
  { file: 'sdk/sdk.toml', readers: ['tests/regression/runtime/upload-kit.ts'] },
  // synth.ts reads the machine types
  { file: 'modules/core/adapters/machines/types.toml', readers: ['tests/regression/runtime/synth.ts'] },
];
export const TEST_READS = [
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
];
// Full keys list every shared file. A merge key lists a file only when the test (its import closure) reads it, or runs patchkit
// live: the other effects of the file are in the bytes the key hashes.
const NON_CELL = nonCellGlobs();
const NODE_REGISTRY = workspaceFile('packages/devices/src/node.ts');
const sharedReads = (imports = null, patchkitLive = false) => {
  const files = SHARED_READS.filter(
    ({ readers }) =>
      !imports || !readers || patchkitLive || readers.some((reader) => imports.includes(workspaceFile(reader))),
  ).map(({ file }) => workspaceFile(file));
  // devices without a cell are no input, unless the test loads the whole registry (packages/devices node.ts reads every file)
  const registry = imports?.includes(NODE_REGISTRY);
  return registry ? files : [...files, ...NON_CELL.map((glob) => `!{workspaceRoot}/${glob}`)];
};
export const READ_BY_TESTS = [...sharedReads(), ...TEST_READS];

// Core-change policy. A change runs all device tests of the projects it edits. A device test that a change reaches
// only through shared inputs (other core modules, SDK, device profiles, patchkit, the daemon) runs only if it is
// listed in `metadata.mergeTests` of its project.json (paths relative to the project root). Every other file gets a
// narrow merge key (its own folder, test code and loaded packs), so Nx replays its last result. `mise run test:full`
// runs every file with full inputs and DIGIKIT_TIER=full (`test-device-all`); run it before each push or PR to main.
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
export const DEVICE_COMMON = [
  'jsToolchain',
  { runtime: 'node tools/nx/device-inputs.mjs' },
  { env: 'DIGI_EMU_FAST' },
  { env: 'DIGI_NO_PROGRESS_MS' },
  // quick or full tier (tests/regression/runtime/devices.ts FULL_TIER): selects which tests run.
  { env: 'DIGIKIT_TIER' },
  { env: 'DIGI_PK_READY_DT2_117' },
  { env: 'DIGI_PK_READY_DN2_112' },
  { env: 'SHARC_FAST_REGIONS' },
  { env: 'SHARC_FAST_REGIONS_2' },
  { env: 'SHARC_BLOCKTRANS' },
  { env: 'SHARC_BLOCKTRANS_2' },
  { env: 'SHARC_FAST_FALLBACK_REGIONS' },
  { env: 'SHARC_FAST_FALLBACK_REGIONS_2' },
  { env: 'SHARC_FAST_AUTO' },
  { env: 'UPDATE_GOLDENS' },
  '{workspaceRoot}/config/emulator.json',
];
// Full set: what the run consumes is the built bytes (the image hashes, the packs, the tool binaries), not the sources
// they are made from. A source edit that leaves these bytes alone changes no key.
export const outputFiles = (globs) => globs.map((glob) => ({ dependentTasksOutputFiles: glob, transitive: true }));
const SHARED_BUILT = [
  'dist/bytes/core-objects.sha256',
  'dist/modules/*/*.dkm',
  'dist/modules/*/*.dks',
  'dist/ports/*.dkport',
  'dist/ports-test/*.dkport',
  'dist/plusdrive/plusdrive',
  'apps/web/public/dksample.wasm',
];
// The binaries that move with every edit of the patchkit sources, whatever bytes the edit packs: the patchkit CLI, and the daemon
// (dist/daemons.json holds the path of the store entry, which is keyed on the patchkit sources too).
const PATCHKIT_BINARIES = ['dist/patchkit/patchkit', 'dist/daemons.json'];
// What a test result follows when it does not run patchkit itself: the patched images, plans and address maps, and the daemon key
// without the sources that only patchkit reads and without the files of the other devices (`emud_key.test_key_of`, written by
// `verify_build daemons`). Both files are per device: `{device}` is replaced for each half by `forDevice`.
const MERGE_BYTES = ['dist/bytes/images-{device}.sha256', 'dist/bytes/daemon-test-{device}.key'];
export const BUILT_OUTPUTS = outputFiles([...SHARED_BUILT, ...PATCHKIT_BINARIES]);
// Merge keys: the same without the binaries; a test that runs patchkit live (PATCHKIT_LIVE) follows the patchkit key of its device instead
// (tools/nx/patchkit-key.mjs, written by `patchkit:cli`): the sources patchkit was built from, without the other devices' files.
// `{device}` is replaced for each half by `forDevice`.
export const PATCHKIT_TEST_KEY = 'dist/bytes/patchkit-test-{device}.key';
// Stands for the negative globs of the files of the other devices; `forDevice` expands it.
export const OTHER_DEVICE_FILES = '!{workspaceRoot}/{other-device-files}';
export const MERGE_OUTPUTS = outputFiles([...SHARED_BUILT, ...MERGE_BYTES]);
// A test whose import closure holds one of these runs patchkit (every module that executes it imports patchkit.ts, or is listed):
// its key follows patchkit (the patchkit key of the half; the binary only in a full key). Only runtime-plan.ts does not count: it runs
// `apply --set`, and its image, plan and address map are hashed in dist/bytes/images-<device>.sha256. (An explicit module list goes
// through runtime-plan-modules.ts, which imports patchkit.ts.)
export const PATCHKIT_LIVE = ['tests/regression/runtime/patchkit.ts'].map((file) => `{workspaceRoot}/${file}`);
export const DEVICE_INPUTS = [...READ_BY_TESTS, ...BUILT_OUTPUTS, ...DEVICE_COMMON];
// A test whose import closure holds one of these runs `wb transfer`, which loads apps/web/public/patchkit.wasm (the target depends on
// `patchkit:wasm` and the key holds the wasm).
export const PATCHKIT_WASM_USERS = ['tests/regression/runtime/transfer-cli.ts'].map(
  (file) => `{workspaceRoot}/${file}`,
);

export const mergeDeviceInputs = (imports, patchkitLive) => [
  ...sharedReads(imports, patchkitLive),
  ...TEST_READS,
  ...MERGE_OUTPUTS,
  ...(patchkitLive ? outputFiles([PATCHKIT_TEST_KEY]) : []),
  // a half reads the files of its own device; a test that loads the whole registry (node.ts) reads them all
  ...(imports?.includes(NODE_REGISTRY) ? [] : [OTHER_DEVICE_FILES]),
  ...DEVICE_COMMON,
];

// The inputs of one half: `device` (a device short) fills the placeholders of the per-device files and expands the other devices' files
// into negative globs; null (the no-op parent of the halves) takes the files of every device and no exclusion. A full key holds no
// placeholder: unchanged.
export const forDevice = (inputs, device) =>
  inputs.flatMap((input) => {
    if (input === OTHER_DEVICE_FILES) return device ? otherFiles(device).map((glob) => `!{workspaceRoot}/${glob}`) : [];
    const pattern = input?.dependentTasksOutputFiles;
    if (typeof pattern === 'string' && pattern.includes('{device}'))
      return [{ ...input, dependentTasksOutputFiles: pattern.replaceAll('{device}', device ?? '*') }];
    return [input];
  });
// Tests that run `patchkit build` or `pack` at run time, or patch with explicit fixture modules, compile sources:
// they keep the SDK and the toolchain as inputs (record flag `compiles`, `patchModules`, or any Python tool the test names: the Python devtools may build or pack).
export const COMPILE_INPUTS = [...SHARED_SDK_INPUTS, '{workspaceRoot}/tools/patchkit/toolchain/**/*'];
// Directories of a module project that its objects are built from (see the pack and image outputs).
export const builtSourceExclusions = (root, base = '.') =>
  ['src', 'include', 'fit', 'sdk']
    .filter((dir) => existsSync(join(base, root, dir)) && !root.includes('/tests'))
    .map((dir) => `!{projectRoot}/${dir}/**/*`);

// The leaves of the modules with role "service": they pack to `.dks`, every other module to `.dkm`. `byId`: id -> { manifest }.
export const serviceLeaves = (byId) =>
  new Set([...byId].filter(([, { manifest }]) => manifest.role === 'service').map(([id]) => id.split('/').at(-1)));

// A service goes on a card as packed, with the port file of the firmware (`patchkit:ports`, dist/ports): a device test that selects a service depends on that target.
// `serviceIds`: the ids of the modules with role "service".
export const serviceIds = (byId) =>
  new Set([...byId].filter(([, { manifest }]) => manifest.role === 'service').map(([id]) => id));

// The code a device test drives the device with. Its own host tests cover it: merge and narrow keys leave it out;
// full keys (test-device-all, conformance, test:full) keep it.
export const DRIVER_ROOTS = ['packages/dk-session/', 'packages/test-support/', 'packages/control-client/'];
// `imports` in the form of importInputs (`{workspaceRoot}/<path>`) without the files of DRIVER_ROOTS.
export const withoutDriver = (imports) =>
  imports.filter((p) => !DRIVER_ROOTS.some((root) => p.startsWith(`{workspaceRoot}/${root}`)));

// The cache inputs of one device test target, and whether its key follows patchkit (`patchkitLive`).
// `opts`: `narrow` selects the narrow input set; `moduleId` is the project's module id (its pack is loaded when
// `uploadable`); `subjects` are the projects a test-only project tests; `services` is the set of service leaves
// (`serviceLeaves`): a narrow key follows the `.dks` pack of those, the `.dkm` pack of every other leaf.
export const deviceInputs = (
  root,
  tests,
  recs,
  imageInput,
  importInputs,
  pythonInputs,
  uploadable = false,
  opts = {},
) => {
  // The test files and the setup file with everything they import.
  const imports = importInputs([...tests.map((test) => join(root, test)), VITEST_SETUP]);
  // The `.py` tools a record lists in `sources` seed the Python input walk.
  const seeds = [
    ...new Set(
      recs.flatMap((rec) =>
        (rec.sources ?? [])
          .map((s) => s.replace('{workspaceRoot}/', ''))
          .filter((s) => s.startsWith('tools/') && s.endsWith('.py'))
          .map((s) =>
            s
              .slice(0, -3)
              .replace(/\/__init__$/, '')
              .split('/')
              .join('.'),
          ),
      ),
    ),
  ];
  // A test that runs patchkit for its own sake (or patches with explicit fixture modules) depends on the patchkit code, not only on
  // the image bytes: its key follows patchkit.
  const patchkitCompiles = recs.some((rec) => rec.patchModules?.length);
  const patchkitLive = imports.some((file) => PATCHKIT_LIVE.includes(file)) || patchkitCompiles;
  const patchkitWasm = imports.some((file) => PATCHKIT_WASM_USERS.includes(file));
  // the files the records name in `sources`; driver files named there follow the same rule as the imports
  const sources = [...new Set(recs.flatMap((rec) => (rec.sources ?? []).map((s) => `{workspaceRoot}/${s}`)))];
  const driverless = opts.merge || opts.narrow;
  const inputs = [
    ...(patchkitWasm ? [{ dependentTasksOutputFiles: 'apps/web/public/patchkit.wasm', transitive: true }] : []),
    // a test that reads the precomputed `needs` of committed packs keys on that file's content
    ...(imports.includes(PACK_NEEDS) ? outputFiles(['dist/bytes/pack-verdicts.json']) : []),
    ...(opts.narrow && patchkitLive ? outputFiles([PATCHKIT_TEST_KEY]) : []),
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
            dependentTasksOutputFiles: `dist/modules/*/${leaf}.${opts.services?.has(leaf) ? 'dks' : 'dkm'}`,
            transitive: true,
          })),
          // the port files that go on the card with a service (the fixture service takes the port file of the test image)
          ...(recs.some((r) => (r.modules ?? []).some((id) => opts.services?.has(id.split('/').at(-1))))
            ? [
                { dependentTasksOutputFiles: 'dist/ports/*.dkport', transitive: true },
                { dependentTasksOutputFiles: 'dist/ports-test/*.dkport', transitive: true },
              ]
            : []),
          // tools the test runs directly
          ...(recs.some((r) => r.plusdrive)
            ? [{ dependentTasksOutputFiles: 'dist/plusdrive/plusdrive', transitive: true }]
            : []),
          ...(recs.some((r) => r.dksampleWasm)
            ? [{ dependentTasksOutputFiles: 'apps/web/public/dksample.wasm', transitive: true }]
            : []),
          ...(recs.some((r) => r.dkprofWasm)
            ? [{ dependentTasksOutputFiles: 'apps/web/public/dkprof.wasm', transitive: true }]
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
    ...(driverless ? withoutDriver(imports) : imports),
    ...(!opts.narrow && (patchkitCompiles || recs.some((rec) => rec.compiles || seeds.length)) ? COMPILE_INPUTS : []),
    ...(driverless ? withoutDriver(sources) : sources),
    ...(seeds.length ? pythonInputs(seeds) : []),
  ];
  return { inputs, patchkitLive, patchkitWasm };
};

// Files that every device test's key holds (the shared reads, the test reads, the SDK and the toolchain), as repository globs.
// `test:subset --base` cannot select by path for a change in them (tools/nx/test-subset.mjs).
export const NON_CELL_GLOBS = NON_CELL;
export const SHARED_INPUT_GLOBS = [...SHARED_READS.map(({ file }) => file), ...TEST_READS, ...COMPILE_INPUTS].map(
  (glob) => glob.replace('{workspaceRoot}/', ''),
);

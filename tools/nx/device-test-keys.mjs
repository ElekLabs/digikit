// What a device test's cache key contains. It does not hold the shape of the targets (device-tests.mjs).
// Three keys exist:
// - full key (DEVICE_INPUTS): all shared reads plus the built outputs, including the patchkit binary;
// - merge key: only the bytes the test loads (the image hashes and the daemon key of its device, its packs) plus the reads its
//   import closure needs, without the files of the other devices; used for the files not listed in metadata.mergeTests;
// - narrow key: the same bytes (image hashes, daemon key), the packs the test loads, and its imports.
// Merge and narrow keys hold only the artifacts the test loads, its own test files and setup (`ownFiles`), its records' `sources`,
// its import closure and DRIVER_SUBJECT_FILES; the conformance merge key is built by the same function (`deviceInputs`).
// Builds key on sources, tests key on what they run: no merge or narrow key holds a source file of the project or its subject.
// Python tool inputs: the import closure (py-imports.mjs) of the `.py` tools a record lists in its `sources`. A test that
// runs a new Python tool must list it there.
import { existsSync, globSync, readdirSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { noDspShorts, nonCellGlobs, otherFiles } from './device-files.mjs';
import { testSourceGlobs } from './test-source.mjs';
import { SHARED_SDK_INPUTS } from './module-build.mjs';

export const PACK_NEEDS = '{workspaceRoot}/tests/regression/runtime/pack-needs.ts';

// Files that tests read by path and do not import. The import walk (ts-imports.mjs) cannot see them.
// Extend this list when a test reads a new file; a file only one record's tests read goes in that record's `sources`.
// SHARED_READS are files that every device test may read and the built bytes do not carry.
export const workspaceFile = (file) => `{workspaceRoot}/${file}`;
const SHARED_READS = [
  // tests/regression/runtime/device-data.ts and card-cache.ts read the profiles and the firmware ports
  { file: 'devices/**/*.toml' },
  { file: 'ports/*.toml' },
  { file: 'ports/fw/**/*.toml' },
  // the manifests of the core modules: `patchkit apply` reads them with the objects (the image, plan and address map in
  // dist/bytes/images-<device>.sha256 carry the effect); only the memaudit helper reads them itself
  { file: '{runtime,services,image-modules}/**/module.toml', readers: ['tests/regression/runtime/memaudit-kit.ts'] },
  // upload-kit.ts reads the runtime version and the `since` of the imports
  { file: 'sdk/sdk.toml', readers: ['tests/regression/runtime/upload-kit.ts'] },
  // synth.ts reads the machine types
  { file: 'image-modules/points/machines/types.toml', readers: ['tests/regression/runtime/synth.ts'] },
];
export const TEST_READS = [
  // fixture files
  '{workspaceRoot}/runtime/core/tests/fixtures/safe-*.bin',
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
// - the module itself (`production` of the project: its sources, without tests/ and docs) in a full key only;
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
  // the sidecars of a pack (`<leaf>.services.json`, `<leaf>.addrmap.json`): tests read them besides the pack
  'dist/modules/*/*.services.json',
  'dist/modules/*/*.addrmap.json',
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
const DAEMON_TEST_KEY = 'dist/bytes/daemon-test-{device}.key';
const DAEMON_NODSP_HASH = 'dist/bytes/daemon-nodsp.sha256';
const MERGE_BYTES = ['dist/bytes/images-{device}.sha256', DAEMON_TEST_KEY];
export const BUILT_OUTPUTS = outputFiles([...SHARED_BUILT, ...PATCHKIT_BINARIES]);
// Merge keys: the same without the binaries; a test that runs patchkit live (PATCHKIT_LIVE) follows the patchkit key of its device instead
// (tools/nx/patchkit-key.mjs, written by `patchkit:cli`): the sources patchkit was built from, without the other devices' files.
// `{device}` is replaced for each half by `forDevice`.
export const PATCHKIT_TEST_KEY = 'dist/bytes/patchkit-test-{device}.key';
// Stands for a subject built into the device images: `forDevice` takes its fileset and image hashes on the halves whose image holds it.
const IMAGE_SUBJECT = Symbol('image-subject');
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
    const subject = input?.[IMAGE_SUBJECT];
    if (subject)
      return forDevice(
        device === null || subject.devices.includes(device)
          ? [...subject.inImage, ...(device === null ? subject.built : [])]
          : subject.built,
        device,
      );
    if (input === OTHER_DEVICE_FILES) return device ? otherFiles(device).map((glob) => `!{workspaceRoot}/${glob}`) : [];
    const pattern = input?.dependentTasksOutputFiles;
    if (typeof pattern === 'string' && pattern.includes('{device}')) {
      // a device without a SHARC DSP has no daemon-test-<device>.key (`verify_build daemons` builds the DSP devices): its daemon is the
      // build of `digi-control:daemon-nodsp`, and the half follows the hash of that binary
      const file =
        device && noDspShorts().includes(device) && pattern === DAEMON_TEST_KEY ? DAEMON_NODSP_HASH : pattern;
      return [{ ...input, dependentTasksOutputFiles: file.replaceAll('{device}', device ?? '*') }];
    }
    return [input];
  });
// Tests that run `patchkit build` or `pack` at run time, or patch with explicit fixture modules, compile sources:
// they keep the SDK and the toolchain as inputs (record flag `compiles`, `patchModules`, or any Python tool the test names: the Python devtools may build or pack).
export const COMPILE_INPUTS = [...SHARED_SDK_INPUTS, '{workspaceRoot}/tools/patchkit/toolchain/**/*'];
// The pack outputs of module ids (`.dks` for a service leaf, else `.dkm`; `services`: the set of `serviceLeaves`).
export const packOutputs = (ids, services) =>
  [...new Set(ids.map((id) => id.split('/').at(-1)))].flatMap((leaf) =>
    [services?.has(leaf) ? 'dks' : 'dkm', 'services.json', 'addrmap.json'].map((ext) => ({
      dependentTasksOutputFiles: `dist/modules/*/${leaf}.${ext}`,
      transitive: true,
    })),
  );
// Directories of a module project that its objects are built from (see the pack and image outputs).
export const builtSourceExclusions = (root, base = '.') =>
  ['src', 'include', 'fit', 'sdk']
    .filter((dir) => existsSync(join(base, root, dir)) && !root.includes('/tests'))
    .map((dir) => `!{projectRoot}/${dir}/**/*`);

// The files of a test-only project that is a tests directory (`runtime/core/tests`): its setup and fixtures. A module project
// holds none: its tests reach the run through the import closure and the records' `sources`.
export const ownFiles = (root, base = '.') =>
  root.split('/').at(-1) === 'tests' || root.includes('/tests/')
    ? [`{workspaceRoot}/${root}/**/*`, ...nestedProjectDirs(root, base).map((d) => `!{workspaceRoot}/${root}/${d}/**`)]
    : [];

// The directories below `root` that hold a project.json: nested projects keep their own files and key. Scanned on every
// graph build (no cache: a long-lived Nx daemon must see a project added since); node_modules is not walked.
const nestedProjectDirs = (root, base) =>
  globSync('**/project.json', { cwd: join(base, root), exclude: (path) => basename(path) === 'node_modules' })
    .map((f) => dirname(f))
    .filter((d) => d !== '.')
    .sort();

// What a test-only project hashes of a module subject that is built into the image: its files without the directories its
// objects are built from (the image hashes hold those). `{ input: 'production', projects }` cannot take exclusions, so the
// subject is a plain fileset: Nx evaluates it per hash, a directory added later is in the key without a graph rebuild.
// The exclusions of nested projects (project.json) and crates (Cargo.toml) are computed here, at graph time: a new project adds a
// project.json and rebuilds the graph. Tests, `*.md` and test-named Rust files stay out like in `production`.
export const subjectInputs = (root, base = '.', keep = null) => {
  const nested = globSync('**/{project.json,Cargo.toml}', { cwd: join(base, root) })
    .map((f) => dirname(f))
    .filter((d) => d !== '.' && !['src', 'include', 'fit', 'sdk', 'build', 'tests'].includes(d.split('/')[0]));
  // Exclusions are global in Nx: a negated `tests` glob would also drop the files of a test project that sits inside the
  // subject (`runtime/core/tests/capacity` in `runtime/core`), its own test files and setup. For such a project the
  // test directories are excluded entry by entry, leaving the path to `keep` (its root, relative to the subject) out.
  // Nx cannot re-include a path below a negated glob (checked with `show target inputs --check`: the exclusion wins), so the
  // listing is a snapshot taken when the graph is built. A plain file or directory added under a test directory of the subject
  // stays an input of the nested project's key (over-selection, never a missed test) until the graph rebuilds, which any
  // project.json or module.toml change does, and a new test file is registered in one.
  const inside = keep?.startsWith(`${root}/`) ? keep.slice(root.length + 1) : null;
  const around = (dir) => {
    const here = join(base, root, dir);
    return readdirSync(here, { withFileTypes: true }).flatMap((e) => {
      const rel = `${dir}/${e.name}`;
      if (rel === inside) return [];
      if (inside.startsWith(`${rel}/`)) return around(rel);
      return [`!{workspaceRoot}/${root}/${rel}${e.isDirectory() ? '/**' : ''}`];
    });
  };
  const testDirs = inside
    ? globSync('**/tests', { cwd: join(base, root) }).filter(
        (d) => !d.startsWith('node_modules/') && !d.includes('/node_modules/'),
      )
    : [];
  const carved = testDirs.flatMap((d) =>
    d === inside ? [] : inside.startsWith(`${d}/`) ? around(d) : [`!{workspaceRoot}/${root}/${d}/**`],
  );
  return [
    `{workspaceRoot}/${root}/**/*`,
    ...['src', 'include', 'fit', 'sdk', 'build', ...(inside ? [] : ['tests'])].map(
      (d) => `!{workspaceRoot}/${root}/${d}/**`,
    ),
    ...[...new Set(nested)].sort().map((d) => `!{workspaceRoot}/${root}/${d}/**`),
    // nested test projects below `tests` (not the project itself, nor a directory it sits in)
    ...(inside
      ? nestedProjectDirs(root, base)
          .filter((d) => d.startsWith('tests/') && d !== inside && !inside.startsWith(`${d}/`))
          .map((d) => `!{workspaceRoot}/${root}/${d}/**`)
      : []),
    `!{workspaceRoot}/${root}/**/*.md`,
    // a pack stages its crate from the patchkit sources; no test reads the manifests
    `!{workspaceRoot}/${root}/Cargo.{toml,lock}`,
    ...(inside
      ? [...new Set(carved)].sort().concat(
          // the rule's globs without the one of the `tests` directory: a path segment of the pattern, not a text match
          testSourceGlobs(`{workspaceRoot}/${root}`).filter(
            (g) => !g.slice(`!{workspaceRoot}/${root}/`.length).split('/').includes('tests'),
          ),
        )
      : testSourceGlobs(`{workspaceRoot}/${root}`)),
  ];
};

// The leaves of the modules with role "service": they pack to `.dks`, every other module to `.dkm`. `byId`: id -> { manifest }.
export const serviceLeaves = (byId) =>
  new Set([...byId].filter(([, { manifest }]) => manifest.role === 'service').map(([id]) => id.split('/').at(-1)));

// A service goes on a card as packed, with the port file of the firmware (`patchkit:ports`, dist/ports): a device test that selects a service depends on that target.
// `serviceIds`: the ids of the modules with role "service".
export const serviceIds = (byId) =>
  new Set([...byId].filter(([, { manifest }]) => manifest.role === 'service').map(([id]) => id));

// The code a device test drives the device with. Its own host tests cover it: merge and narrow keys leave it out;
// full keys (test-device-all, conformance, test:full) keep it. Exception: DRIVER_SUBJECT_FILES.
export const DRIVER_ROOTS = ['packages/dk-session/', 'packages/test-support/', 'packages/control-client/'];
// Files under the driver roots that stay in every key (merge, narrow, full): the protocol and log layout, the stock
// MidiRpc protocol, the emulator launch and its settings, the panel key codes. A change here changes what a device test sends or reads.
export const DRIVER_SUBJECT_FILES = [
  'packages/dk-session/src/gen_proto.ts',
  'packages/dk-session/src/gen_bulk.ts',
  'packages/dk-session/src/runtime-log.ts',
  'packages/dk-session/src/log-names.ts',
  'packages/dk-session/src/runtime-events.ts',
  'packages/dk-session/src/rpcdata.ts',
  'packages/dk-session/src/rpcfiles.ts',
  'packages/test-support/src/session.ts',
  'packages/test-support/src/emulator-config.ts',
  'packages/test-support/src/keys.ts',
  'packages/test-support/src/input.ts',
];
// `imports` in the form of importInputs (`{workspaceRoot}/<path>`) without the files of DRIVER_ROOTS, except DRIVER_SUBJECT_FILES.
export const withoutDriver = (imports) =>
  imports.filter(
    (p) =>
      DRIVER_SUBJECT_FILES.some((f) => p === `{workspaceRoot}/${f}`) ||
      !DRIVER_ROOTS.some((root) => p.startsWith(`{workspaceRoot}/${root}`)),
  );

// The cache inputs of one device test target, and whether its key follows patchkit (`patchkitLive`).
// `opts`: `testPaths` (workspace paths) replaces the test files of the project; `own` replaces its own files; `siblingTests` lists the other test files of the project (names relative to its root), which a project-wide set leaves out (the own test file stays in); `narrow` selects the narrow input set; `moduleId` is the project's module id (its pack is loaded when
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
  const imports = importInputs([...(opts.testPaths ?? tests.map((test) => join(root, test))), VITEST_SETUP]);
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
  const patchkitLive = opts.patchkitLive || imports.some((file) => PATCHKIT_LIVE.includes(file)) || patchkitCompiles;
  const patchkitWasm = imports.some((file) => PATCHKIT_WASM_USERS.includes(file));
  // the files the records name in `sources`; driver files named there follow the same rule as the imports
  const sources = [...new Set(recs.flatMap((rec) => (rec.sources ?? []).map((s) => `{workspaceRoot}/${s}`)))];
  const driverless = opts.merge || opts.narrow;
  const own = driverless ? (opts.own ?? ownFiles(root, opts.base)) : null;
  const wideProject = !driverless || own.length > 0;
  const inputs = [
    ...(patchkitWasm ? [{ dependentTasksOutputFiles: 'apps/web/public/patchkit.wasm', transitive: true }] : []),
    // a test that reads the precomputed `needs` of committed packs keys on that file's content
    ...(imports.includes(PACK_NEEDS) ? outputFiles(['dist/bytes/pack-verdicts.json']) : []),
    ...(opts.narrow && patchkitLive ? outputFiles([PATCHKIT_TEST_KEY]) : []),
    ...(opts.narrow
      ? [
          ...DEVICE_COMMON,
          // device and port data (and, for a patchkit-live test, the core manifests patchkit reads)
          ...sharedReads(imports, patchkitLive),
          // a half reads the files of its own device; a test that loads the whole registry (node.ts) reads them all
          ...(imports?.includes(NODE_REGISTRY) ? [] : [OTHER_DEVICE_FILES]),
          ...TEST_READS,
          // the packs the test loads, not every pack output
          // (the own module, the records' modules and their alias packs)
          ...packOutputs(
            [
              ...(uploadable && opts.moduleId ? [opts.moduleId] : []),
              ...recs.flatMap((r) => [...(r.modules ?? []), ...Object.keys(r.aliases ?? {})]),
            ],
            opts.services,
          ),
          // the image hashes and the daemon key of the half
          ...outputFiles(MERGE_BYTES),
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
          // the subject module of a test-only project: its files that are not built, and the bytes it builds to (the image
          // hashes above, or its pack)
          ...(opts.subjects ?? []).map((id) => {
            const subject = opts.subjectRoots?.[id];
            const production = { input: 'production', projects: [id] };
            if (!subject) return production;
            const files = subjectInputs(subject.root, opts.base, root);
            const pack = subject.pack ? packOutputs([id], opts.services) : [];
            // `forDevice` picks per half: a half whose image holds the subject follows the image hashes; the others follow the
            // pack, or the subject's production files when it has none
            return {
              [IMAGE_SUBJECT]: {
                devices: subject.devices,
                inImage: [...files, ...pack],
                built: subject.pack ? [...files, ...pack] : [production],
              },
            };
          }),
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
    // a full key holds the module itself (its files without tests/ and docs, and without the directories its objects are built
    // from); a merge or narrow key holds the test's own files and setup (`ownFiles`): the objects reach the run through the
    // pack outputs and the image hashes, no other source file is an input
    ...(driverless
      ? (opts.own ?? ownFiles(root, opts.base))
      : ['production', ...builtSourceExclusions(root, opts.base)]),
    ...(driverless ? withoutDriver(imports) : imports),
    // a record that compiles at run time (`compiles`) keeps the SDK and the toolchain in every key, the narrow one included
    ...(recs.some((rec) => rec.compiles) || (!opts.narrow && (patchkitCompiles || seeds.length)) ? COMPILE_INPUTS : []),
    ...(driverless ? withoutDriver(sources) : sources),
    ...(seeds.length ? pythonInputs(seeds) : []),
    // `siblingTests`: the target of one test file leaves the other test files of the project out of a project-wide set
    ...(opts.siblingTests?.length && wideProject ? opts.siblingTests.map((t) => `!{workspaceRoot}/${root}/${t}`) : []),
  ];
  return { inputs, patchkitLive, patchkitWasm };
};

// Files that every device test's key holds (the shared reads, the test reads, the SDK and the toolchain), as repository globs.
// `test:subset --base` cannot select by path for a change in them (tools/nx/test-subset.mjs).
export const NON_CELL_GLOBS = NON_CELL;
export const SHARED_INPUT_GLOBS = [...SHARED_READS.map(({ file }) => file), ...TEST_READS, ...COMPILE_INPUTS].map(
  (glob) => glob.replace('{workspaceRoot}/', ''),
);

// Nx plugin: one project per Cargo workspace member (read from Cargo.toml files, cargo is not run).
// Edges: path dependencies between members, in every dependency table.
// rust-tests is cached for HERMETIC crates (direct cargo nextest into dist/cargo-tests); the others run
// through tools/nx/rust-env.mjs uncached.
import { existsSync, readFileSync } from 'node:fs';
import { join, normalize } from 'node:path';
import { DependencyType } from '@nx/devkit';
import { parse } from 'smol-toml';
import { testSourceGlobs } from './test-source.mjs';

// The packs whose `needs` verdict tests read from dist/bytes/pack-verdicts.json (one list: tests/regression/runtime/needs-packs.json).
const NEEDS_PACKS = JSON.parse(
  readFileSync(new URL('../../tests/regression/runtime/needs-packs.json', import.meta.url), 'utf8'),
).modules;

const read = (root, toml) => parse(readFileSync(join(root, toml), 'utf8'));
const KINDS = ['dependencies', 'dev-dependencies', 'build-dependencies'];

// CLI builds into dist/ (gitignored). Copy then rename: a running process keeps the old file.
const CLI = {
  patchkit: {
    build: 'cargo build -p patchkit --features record-rules --release --offline --target-dir dist/cargo',
    bin: 'patchkit',
    dir: 'patchkit',
    key: 'node tools/nx/patchkit-key.mjs', // dist/bytes/patchkit-test-<device>.key, the key of live device tests
    // The test-source rule is compiled into patchkit (include_str!) and read by the key script.
    inputs: [
      'default',
      '^default',
      '{workspaceRoot}/config/test-source-patterns.json',
      '{workspaceRoot}/tools/nx/test-source.mjs',
    ],
  },
  'plusdrive-format': {
    build: 'cargo build -p plusdrive-format --bin plusdrive --release --locked --offline --target-dir dist/cargo',
    bin: 'plusdrive',
    dir: 'plusdrive',
  },
};
const cliTarget = ({ build, bin, dir, key, inputs }) => ({
  executor: 'nx:run-commands',
  cache: false,
  ...(inputs && { inputs }),
  parallelism: true, // cargo locks dist/cargo itself and the install step is cp+mv, so it may run beside other tasks
  outputs: [`{workspaceRoot}/dist/${dir}/${bin}`, ...(key ? ['{workspaceRoot}/dist/bytes/patchkit-test-*.key'] : [])],
  options: {
    commands: [
      build,
      `mkdir -p dist/${dir} && cp dist/cargo/release/${bin} dist/${dir}/${bin}.tmp && mv -f dist/${dir}/${bin}.tmp dist/${dir}/${bin}`,
      ...(key ? [key] : []),
    ],
    parallel: false,
    cwd: '.',
  },
});

// Extra targets of named crates.
const CRATE_TARGETS = {
  'sharc-rig': {
    build: {
      executor: 'nx:run-commands',
      cache: false, // cargo decides; the hash file it writes is the cache key of the tasks that run the rig
      parallelism: true,
      description:
        'Builds the SHARC-only rig against the shared generated core into dist/bin/sharc-rig and writes dist/bytes/sharc-rig.sha256 (tools/nx/sharc-rig-bin.mjs).',
      inputs: [
        'default',
        '^production',
        '{workspaceRoot}/Cargo.toml',
        '{workspaceRoot}/Cargo.lock',
        '{workspaceRoot}/tools/nx/sharc-rig-bin.mjs',
        '{workspaceRoot}/tools/nx/rust-env.mjs',
        '{workspaceRoot}/out/native/shared-core/gen/CORE_FINGERPRINT',
      ],
      outputs: ['{workspaceRoot}/dist/bin/sharc-rig', '{workspaceRoot}/dist/bytes/sharc-rig.sha256'],
      options: { command: 'node tools/nx/sharc-rig-bin.mjs', cwd: '.' },
    },
  },
  patchkit: {
    'device-bytes': {
      executor: 'nx:run-commands',
      cache: false, // runs every time (under a second): the file it writes is part of the cache key of the device tasks
      parallelism: true,
      description:
        'Writes dist/bytes/core-objects.sha256: the hash of each committed core object without its source hash.',
      inputs: [
        '{workspaceRoot}/tools/nx/device-bytes.mjs',
        '{workspaceRoot}/tools/nx/tracked-objects.mjs',
        '{workspaceRoot}/tools/nx/module-manifests.mjs',
        '{workspaceRoot}/tools/nx/object-hash.mjs',
        { dependentTasksOutputFiles: '**/*', transitive: true },
      ],
      dependsOn: [{ target: 'build-objects', projects: ['tag:acceptance-image'] }],
      outputs: ['{workspaceRoot}/dist/bytes/core-objects.sha256'],
      options: { command: 'node tools/nx/device-bytes.mjs', cwd: '.' },
    },
    'runtime-owners': {
      executor: 'nx:run-commands',
      cache: true,
      parallelism: true,
      description:
        'Writes dist/bytes/owners/: one hash per owner of the runtime object (`patchkit owners`, runtime/core/owners.toml), the shared set, the other runtime objects and the whole object.',
      inputs: [
        '{workspaceRoot}/tools/nx/runtime-owners.mjs',
        '{workspaceRoot}/tools/nx/object-hash.mjs',
        '{workspaceRoot}/runtime/core/owners.toml',
        '{workspaceRoot}/runtime/core/build/*.dko.json',
        { dependentTasksOutputFiles: 'dist/patchkit/patchkit', transitive: true },
      ],
      dependsOn: ['cli'],
      outputs: ['{workspaceRoot}/dist/bytes/owners'],
      options: { command: 'node tools/nx/runtime-owners.mjs', cwd: '.' },
    },
    'pack-verdicts': {
      executor: 'nx:run-commands',
      cache: false, // under a second: the file it writes is part of the key of the device tasks that read it
      parallelism: true,
      description:
        'Writes dist/bytes/pack-verdicts.json: the `patchkit needs` verdict of each pack in tests/regression/runtime/needs-packs.json.',
      inputs: [
        '{workspaceRoot}/tools/nx/pack-verdicts.mjs',
        '{workspaceRoot}/tests/regression/runtime/needs-packs.json',
        { dependentTasksOutputFiles: '**/*', transitive: true },
      ],
      dependsOn: ['cli', ...NEEDS_PACKS.map((id) => `${id}:pack`)],
      outputs: ['{workspaceRoot}/dist/bytes/pack-verdicts.json'],
      options: { command: 'node tools/nx/pack-verdicts.mjs', cwd: '.' },
    },
    'device-images': {
      executor: 'nx:run-commands',
      cache: false, // a few seconds: the file it writes is part of the merge key of the device tasks
      parallelism: true,
      description:
        'Writes dist/bytes/images.sha256 and dist/bytes/images-<device>.sha256: the hash of the image `patchkit apply` makes from each acceptance firmware with each patch set, in one file for all devices and one per device.',
      inputs: [
        '{workspaceRoot}/tools/nx/device-images.mjs',
        '{workspaceRoot}/tools/nx/image-jobs.mjs',
        '{workspaceRoot}/tools/nx/device-inputs.mjs',
        '{workspaceRoot}/tests/regression/runtime/image-roots.json',
        { dependentTasksOutputFiles: '**/*', transitive: true },
      ],
      dependsOn: ['cli', { target: 'build-objects', projects: ['tag:acceptance-image'] }],
      outputs: [
        '{workspaceRoot}/dist/bytes/images.sha256',
        '{workspaceRoot}/dist/bytes/images-*.sha256',
        '{workspaceRoot}/dist/bytes/images',
      ],
      options: { command: 'node tools/nx/device-images.mjs', cwd: '.' },
    },
  },
  dkprof: {
    wasm: {
      executor: 'nx:run-commands',
      cache: false,
      description: 'Builds the dkprof wasm that wb (profile, addrmap and module commands) loads.',
      outputs: ['{workspaceRoot}/apps/web/public/dkprof.wasm'],
      options: { command: 'node tools/nx/wasm.mjs --dkprof-only', cwd: '.' },
    },
  },
  dksample: {
    wasm: {
      executor: 'nx:run-commands',
      cache: false,
      description: 'Builds the dksample wasm that sample device tests load.',
      outputs: ['{workspaceRoot}/apps/web/public/dksample.wasm'],
      options: { command: 'node tools/nx/wasm.mjs --dksample-only', cwd: '.' },
    },
  },
};

// The sources the daemon is built from (SOURCE_GLOBS of tools/devtools/emud_key.py, without the Python tools that run the build).
// The store keys the binary by content; these inputs make a source edit select the task, and so every task that consumes the binary.
// This is the one hand list, for the `daemons` target, whose Python recipe (emud_key.py SOURCE_GLOBS) hashes the same files: change both
// together. `daemon-nodsp` does not use it: it builds one crate, so its inputs are the crate graph (`default`, `^default`) plus the
// extra files in DAEMON_NODSP_EXTRA, checked against the files cargo read (`node tools/nx/cargo-depfiles.mjs`).
const DAEMON_BUILD_SOURCES = [
  '{workspaceRoot}/Cargo.toml',
  '{workspaceRoot}/Cargo.lock',
  '{workspaceRoot}/rust-toolchain*',
  '{workspaceRoot}/.cargo/**/*',
  '{workspaceRoot}/packages/emulator/**/*',
  // The emulator guide is developer documentation and is never read by the daemon build.
  '!{workspaceRoot}/packages/emulator/README.md',
  '{workspaceRoot}/packages/dkaddrmap/**/*',
  '{workspaceRoot}/packages/dkprof/**/*',
  '{workspaceRoot}/packages/dksharc/**/*',
  '{workspaceRoot}/packages/dkneeds/**/*',
  '{workspaceRoot}/packages/dksample/**/*',
  '{workspaceRoot}/packages/dkupload/**/*',
  '{workspaceRoot}/packages/patchkit/**/*',
  '{workspaceRoot}/runtime/core/abi/{Cargo.toml,src/**/*}',
  '{workspaceRoot}/runtime/core/fit/{Cargo.toml,build.rs,src/**/*}',
  '{workspaceRoot}/runtime/core/sdk/{Cargo.toml,build.rs,src/**/*}',
  // Test sources (config/test-source-patterns.json): `#[cfg(test)]` modules that the release build does not compile.
  ...testSourceGlobs('{workspaceRoot}/{packages,modules,runtime,services,ports/bindings}', '.rs'),
  '{workspaceRoot}/devices/**/*',
  '{workspaceRoot}/ports/*.toml',
  '{workspaceRoot}/ports/fw/**/*',
  '{workspaceRoot}/tests/scenarios/*.json', // read by include_str! in the emulator crates
  '{workspaceRoot}/sdk/sdk.toml',
  '{workspaceRoot}/runtime/core/abi.toml',
  '{workspaceRoot}/tools/patchkit/toolchain/**/*',
  '{workspaceRoot}/tools/sharc_dev/sharc_core/seed_tables.py',
  '{workspaceRoot}/config/test-source-patterns.json',
];

// Files outside the crates that the digi-emud build reads (include_str!, build scripts) and what runs it. The crate files come from the
// project graph (path dependencies of digi-control).
export const DAEMON_NODSP_EXTRA = [
  '{workspaceRoot}/Cargo.toml',
  '{workspaceRoot}/Cargo.lock',
  '{workspaceRoot}/rust-toolchain*',
  '{workspaceRoot}/.cargo/**/*',
  '{workspaceRoot}/mise.toml',
  '{workspaceRoot}/tools/nx/cargo.mjs',
  '{workspaceRoot}/tools/nx/rust-env.mjs',
  '{workspaceRoot}/devices/**/*',
  '{workspaceRoot}/ports/*.toml',
  '{workspaceRoot}/ports/fw/**/*',
  '{workspaceRoot}/sdk/sdk.toml',
  '{workspaceRoot}/runtime/core/abi.toml',
  '{workspaceRoot}/config/test-source-patterns.json',
  '{workspaceRoot}/tools/patchkit/toolchain/**/*',
  '{workspaceRoot}/tests/scenarios/*.json', // read by include_str! in the emulator crates
];

// Gate helper targets. They build under dist/ or out/native; out/ is never an Nx output.
const GATE = {
  'digi-control': {
    daemons: {
      executor: 'nx:run-commands',
      cache: false,
      parallelism: true, // no gate task is exclusive; an exclusive task scheduled mid-run makes Nx wait for every running task
      outputs: [
        '{workspaceRoot}/dist/daemons.json',
        '{workspaceRoot}/dist/bytes/daemon-test.key',
        '{workspaceRoot}/dist/bytes/daemon-test-*.key',
      ],
      inputs: [
        ...DAEMON_BUILD_SOURCES,
        '{workspaceRoot}/tools/devtools/emud_key.py',
        '{workspaceRoot}/tools/devtools/verify_build.py',
        '{workspaceRoot}/tools/devtools/stablebin.py',
        '{workspaceRoot}/tools/devtools/store.py',
        '{workspaceRoot}/tools/devtools/core_store.py',
        '{workspaceRoot}/tools/emulator/digi_env.py',
        '{workspaceRoot}/tools/emulator/native_pgo.py',
        '{workspaceRoot}/tools/emulator/native_target.py',
        '{workspaceRoot}/tools/emulator/native_emu.py',
        '{workspaceRoot}/tools/patchkit/pk_emud_pgo.py',
      ],
      options: {
        commands: [
          'mkdir -p dist',
          'CARGO_BUILD_JOBS=${DIGI_JOBS:-4} uv run python -m tools.devtools.verify_build daemons --env-out dist/daemons.json',
        ],
        parallel: false,
        cwd: '.',
      },
    },
    'daemon-nodsp': {
      executor: 'nx:run-commands',
      cache: true,
      parallelism: true,
      description:
        'Builds digi-emud without a SHARC core (the daemon of the devices with `[capabilities] dsp = "none"`, which boot cold) into dist/bin/digi-emud-nodsp: the binary the daemons target builds for them.',
      inputs: ['default', '^default', ...DAEMON_NODSP_EXTRA],
      // the binary and its hash: the key of the device tests of the no-DSP halves (device-test-keys.mjs, instead of daemon-test-<device>.key)
      outputs: ['{workspaceRoot}/dist/bin/digi-emud-nodsp', '{workspaceRoot}/dist/bytes/daemon-nodsp.sha256'],
      options: {
        commands: [
          // the no-core target dir and the cf-profile variant of the daemon (every workbench daemon has the run-time ColdFire profile)
          'CARGO_BUILD_JOBS=${DIGI_JOBS:-4} node tools/nx/rust-env.mjs --variant cfprof -- cargo build --release --locked --manifest-path packages/emulator/control/Cargo.toml --bin digi-emud --features cf-profile',
          `mkdir -p dist/bin && node tools/nx/rust-env.mjs --variant cfprof -- sh -c 'cp "$CARGO_TARGET_DIR/release/digi-emud" dist/bin/digi-emud-nodsp.tmp && mv -f dist/bin/digi-emud-nodsp.tmp dist/bin/digi-emud-nodsp'`,
          'mkdir -p dist/bytes && shasum -a 256 dist/bin/digi-emud-nodsp | cut -d" " -f1 > dist/bytes/daemon-nodsp.sha256',
        ],
        parallel: false,
        cwd: '.',
      },
    },
  },
  'digi-coupled': {
    build: {
      executor: 'nx:run-commands',
      cache: false,
      parallelism: true, // cargo jobs are capped by CARGO_BUILD_JOBS so it can build beside device tests
      // dist/coupled-dn2.json holds the sha256 of the built binary: the cache key of the targets that run it.
      outputs: ['{workspaceRoot}/dist/coupled-dn2.json'],
      options: {
        command:
          'CARGO_BUILD_JOBS=${DIGI_JOBS:-4} uv run python -m tools.emulator.coupled build --id-out dist/coupled-dn2.json',
        cwd: '.',
      },
    },
    'build-dt2': {
      executor: 'nx:run-commands',
      cache: false,
      parallelism: true,
      outputs: ['{workspaceRoot}/dist/coupled-dt2.json'],
      options: {
        command:
          'CARGO_BUILD_JOBS=${DIGI_JOBS:-4} uv run python -m tools.emulator.coupled build --device dt2 --id-out dist/coupled-dt2.json',
        cwd: '.',
      },
    },
  },
};

// Crates without tests; no rust-tests target.
const NO_TESTS = new Set([
  'sharc-stats',
  'sharc-gen',
  'dkfw',
  'dkrt-test-support',
  'sharc-fast-cl',
  'bootstrap-emu',
  'digiemu-desktop',
  'digi-wasm-core',
  'dynlib',
  'sharc-core-lib',
]);

// Crates whose tests read no firmware, generated core or out/ data; their rust-tests are cached.
// Extra inputs are files compiled in (include_str!). Check with `node tools/nx/cargo-depfiles.mjs` after a run.
export const HERMETIC = {
  coldfire: [],
  periph: [],
  'emmc-card': [],
  dkaddrmap: [],
  dkprof: [],
  dkneeds: [],
  dksharc: [],
  'dkrt-sdk': ['{workspaceRoot}/runtime/core/vectors/settings-vectors.json'],
  'dkrt-fit': ['{workspaceRoot}/runtime/core/vectors/fit-vectors.json'],
  'dkrt-lifecycle': [],
  'device-profile': ['{workspaceRoot}/devices/digitakt-ii.toml', '{workspaceRoot}/devices/digitone-ii.toml'],
  dkupload: [],
  dksample: [],
  machine: [],
  'sharc-rt': [],
  dkmidi: [],
  dkbulk: [],
};

// The shared fixtures (`tests/fixtures/modules`) whose `init` the contract suite of a service crate mirrors with a call list (`tests/contract.rs`): a change of a
// fixture selects the crate's tests, so the call list is read against it.
const fixture = (name) => [
  `{workspaceRoot}/tests/fixtures/modules/${name}/module.toml`,
  `{workspaceRoot}/tests/fixtures/modules/${name}/src/**/*`,
  `{workspaceRoot}/tests/fixtures/modules/${name}/build/gen/module_ids.h`,
];
const FIXTURE_SOURCE = {
  'dkrt-svc-lfo': fixture('lfo-shape'),
  'dkrt-svc-fx': [...fixture('dyn-fx-gain'), ...fixture('dyn-fx-gain-b')],
  'dkrt-svc-machines': fixture('sine'),
  'dkrt-svc-shortcuts': fixture('shortcut-demo'),
};

// Files outside the crate embedded by patchkit's source, unit tests and build.rs.
const COMPILE_SOURCE = {
  patchkit: [
    '{workspaceRoot}/sdk/sdk.toml',
    '{workspaceRoot}/runtime/core/abi.toml',
    '{workspaceRoot}/runtime/core/vectors/fit-vectors.json',
    '{workspaceRoot}/runtime/core/vectors/settings-vectors.json',
    '{workspaceRoot}/tests/fixtures/ping-vectors.json', // flash_rpc.rs: `include_str!` in its unit test
    '{workspaceRoot}/devices/*.toml',
    '{workspaceRoot}/tools/patchkit/toolchain/pins.toml',
    '{workspaceRoot}/tools/patchkit/toolchain/m68k-cf-none-elf-a2.json',
    '{workspaceRoot}/tools/patchkit/pk_toolchains.py',
  ],
};

// Normalized repo-relative roots of the path dependencies of a crate (all dependency tables).
export const pathDeps = (workspaceRoot, crateRoot) => {
  const t = read(workspaceRoot, join(crateRoot, 'Cargo.toml'));
  const tables = [t, ...Object.values(t.target ?? {})].flatMap((s) => KINDS.map((k) => s[k] ?? {}));
  const deps = tables.flatMap((tbl) =>
    Object.values(tbl)
      .filter((d) => d?.path)
      .map((d) => normalize(join(crateRoot, d.path))),
  );
  return [...new Set(deps)];
};

// Package names of the path dependencies of a crate, transitively (each is a project of this plugin).
const pathDepNames = (workspaceRoot, crateRoot, seen = new Set()) => {
  for (const dep of pathDeps(workspaceRoot, crateRoot)) {
    const { name } = read(workspaceRoot, join(dep, 'Cargo.toml')).package;
    if (seen.has(name)) continue;
    seen.add(name);
    pathDepNames(workspaceRoot, dep, seen);
  }
  return [...seen].sort();
};

// `rust-tests` of a standalone crate: a crate with its own `[workspace]` and Cargo.lock inside a module directory (the
// service crates `services/*/dks`), so no member of the root workspace. modules.mjs gives it to every module
// project whose root holds such a crate. Uncached like the non-hermetic members. The inputs are the project's own files
// (crate, Cargo.lock, tests), the production files of the projects it depends on by path (listed,
// transitively, from the Cargo.toml path dependencies) and the toolchain. A crate without tests passes (`--no-tests=pass`).
export const standaloneRustTests = (workspaceRoot, root) => {
  const manifest = join(root, 'Cargo.toml');
  if (!existsSync(join(workspaceRoot, manifest)) || !read(workspaceRoot, manifest).workspace) return null;
  return {
    executor: 'nx:run-commands',
    cache: false,
    inputs: [
      ...rustInputs(workspaceRoot),
      { input: 'production', projects: pathDepNames(workspaceRoot, root) },
      '{workspaceRoot}/tools/nx/rust-env.mjs',
    ],
    options: {
      command: `node tools/nx/rust-env.mjs -- cargo nextest run --manifest-path ${manifest} --release --locked --offline --no-tests=pass`,
      cwd: '.',
    },
  };
};

const rustInputs = (workspaceRoot) => {
  const inputs = ['default', '^production', '{workspaceRoot}/Cargo.toml', '{workspaceRoot}/Cargo.lock'];
  if (existsSync(join(workspaceRoot, 'rust-toolchain.toml'))) inputs.push('{workspaceRoot}/rust-toolchain.toml');
  return inputs;
};

export const createNodesV2 = [
  'Cargo.toml',
  (files, _opts, ctx) =>
    files.map((f) => {
      const inputs = rustInputs(ctx.workspaceRoot);
      const rust = read(ctx.workspaceRoot, 'mise.toml').tools.rust;
      const hermeticInputs = [
        'default',
        '^production',
        '{workspaceRoot}/Cargo.toml',
        '{workspaceRoot}/Cargo.lock',
        ...(existsSync(join(ctx.workspaceRoot, 'rust-toolchain.toml')) ? ['{workspaceRoot}/rust-toolchain.toml'] : []),
        { runtime: `rustup run ${rust} rustc -vV` },
        { runtime: 'node tools/nx/cargo-profile-env.mjs' },
        '{workspaceRoot}/tools/nx/cargo-profile-env.mjs',
        { env: 'RUSTFLAGS' },
        { env: 'CARGO_ENCODED_RUSTFLAGS' },
        { env: 'CARGO_BUILD_TARGET' },
        { env: 'RUSTC_WRAPPER' },
        { env: 'RUSTC_WORKSPACE_WRAPPER' },
        { env: 'RUST_TEST_THREADS' },
      ];
      const rustTests = (name) =>
        name in HERMETIC
          ? {
              executor: 'nx:run-commands',
              cache: true,
              inputs: [...hermeticInputs, ...HERMETIC[name]],
              outputs: [],
              options: {
                command: `rustup run ${rust} cargo nextest run -p ${name} --release --locked --offline --target-dir dist/cargo-tests`,
                cwd: '.',
              },
            }
          : {
              executor: 'nx:run-commands',
              cache: false,
              inputs: [
                ...inputs,
                '{workspaceRoot}/tools/nx/rust-env.mjs',
                ...(COMPILE_SOURCE[name] ?? []),
                ...(FIXTURE_SOURCE[name] ?? []),
              ],
              options: {
                command: `node tools/nx/rust-env.mjs${name === 'sharc-native' || name === 'sharc-rig' ? ' --gen' : ''} -- cargo nextest run -p ${name}${name === 'patchkit' ? ' --features record-rules' : ''} --release --locked --offline`,
                cwd: '.',
                ...(name === 'patchkit' && { env: { DIGI_REQUIRE_TOOLCHAINS: '1' } }),
              },
            };
      const projects = {};
      for (const root of read(ctx.workspaceRoot, f).workspace?.members ?? []) {
        const { name } = read(ctx.workspaceRoot, join(root, 'Cargo.toml')).package;
        projects[root] = {
          name,
          root,
          tags: ['rust'],
          targets: {
            ...(!NO_TESTS.has(name) && { 'rust-tests': rustTests(name) }),
            ...(CLI[name] && { cli: cliTarget(CLI[name]) }),
            ...CRATE_TARGETS[name],
            ...GATE[name],
          },
        };
      }
      return [f, { projects }];
    }),
];

export const createDependencies = (_opts, ctx) => {
  const mods = Object.values(ctx.projects).filter((p) => p.tags?.includes('rust'));
  return mods.flatMap((p) => {
    const file = join(p.root, 'Cargo.toml');
    return pathDeps(ctx.workspaceRoot, p.root)
      .map((path) => mods.find((m) => m.root === path)?.name)
      .filter((target) => target && target !== p.name)
      .map((target) => ({ source: p.name, target, sourceFile: file, type: DependencyType.static }));
  });
};

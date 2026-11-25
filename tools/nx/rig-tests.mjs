import { existsSync } from 'node:fs';
import { dirname, join, normalize } from 'node:path';
import {
  CONFORMANCE_ROOTS,
  NEGATIVE_VARIANTS,
  conformanceKind,
  dspSourcePaths,
  negativeVariantsOf,
} from '../../packages/sharc-rig-kit/src/conformance-manifest.mjs';
import { ALL_MANIFESTS, SHARED_SDK_INPUTS } from './module-build.mjs';
import { packDevices, read } from './module-manifests.mjs';
import { sourceIncludeInputs } from './pack-inputs.mjs';

export const RIG_KIT_ROOT = 'packages/sharc-rig-kit';
export const DSP_CONFORMANCE = 'test-dsp-conformance';
export const DSP_CONFORMANCE_ALL = 'test-dsp-conformance-all';
const PINS = 'tools/patchkit/toolchain/pins.toml';

const machineAdapters = [
  '{workspaceRoot}/runtime/platform/dsp-voice/**/*',
  '{workspaceRoot}/runtime/platform/dsp-link/**/*',
  '{workspaceRoot}/services/machines/dks/{dsp,build/gen}/**/*',
  '{workspaceRoot}/runtime/platform/rows/types.toml',
];

const fxAdapters = ['{workspaceRoot}/runtime/platform/fx/**/*', '{workspaceRoot}/services/fx/dks/{dsp,build/gen}/**/*'];

const conformance = ({ root = '', file, inputs, reports, fixed = false, suiteAdapters = [] }) => {
  const command = `pnpm exec vitest run --config vitest.host.config.ts --project sharc-rig-conformance${file ? ` conformance/${file}` : ''}`;
  return {
    executor: 'nx:run-commands',
    cache: fixed,
    dependsOn: ['patchkit:cli', 'sharc-rig:build'],
    inputs: [
      'jsToolchain',
      '{workspaceRoot}/vitest.host.config.ts',
      '{workspaceRoot}/vitest.host.projects.json',
      '{workspaceRoot}/tsconfig*.json',
      `{workspaceRoot}/${RIG_KIT_ROOT}/package.json`,
      'dspTestTooling',
      `{workspaceRoot}/${RIG_KIT_ROOT}/conformance/${file ?? '**/*'}`,
      ...suiteAdapters,
      ...SHARED_SDK_INPUTS,
      '{workspaceRoot}/devices/*.toml',
      '{workspaceRoot}/devices/cycle-factors.json',
      `{workspaceRoot}/${PINS}`,
      { externalDependencies: ['vitest'] },
      { dependentTasksOutputFiles: '**/*' },
      // Report provenance is embedded in the JSON and must match this worktree.
      ...(reports ? [{ runtime: `node -p "require('node:fs').realpathSync('.')"` }] : []),
      ...inputs,
    ],
    outputs: reports ? [`{workspaceRoot}/${reports}`] : [],
    options: {
      forwardAllArgs: false,
      cwd: '.',
      ...(reports
        ? {
            // Clear this target's reports before writing new ones.
            commands: [
              `node --eval "const fs = require('node:fs'), dir = process.env.DIGI_CONFORM_REPORTS; fs.rmSync(dir, { recursive: true, force: true }); fs.mkdirSync(dir, { recursive: true })"`,
              command,
            ],
            parallel: false,
          }
        : { command }),
      ...(fixed
        ? {
            env: {
              DIGI_CONFORM_MODULES: '',
              DIGI_CONFORM_DEVICE: '',
              DIGI_CONFORM_ROOT: root,
              DIGI_CONFORM_REPORTS: reports ?? '',
              DIGI_RIG_ROOT: '',
              PATCHKIT_MODULES: '',
              DIGI_PATCHKIT: '',
              DIGI_SHARC_RIG: '',
            },
          }
        : {}),
    },
  };
};

const fxWriteInput = '{workspaceRoot}/tests/support/fx_effects.s';

// The explicit target accepts user selectors and arbitrary report, workspace and binary overrides.
export const allRigConformance = () =>
  conformance({
    suiteAdapters: [...machineAdapters, ...fxAdapters],
    inputs: [
      ...ALL_MANIFESTS,
      ...CONFORMANCE_ROOTS.flatMap((root) => [
        `{workspaceRoot}/${root}/**/src/**/*.{s,inc}`,
        `{workspaceRoot}/${root}/**/build/gen/**/*`,
      ]),
      fxWriteInput,
    ],
  });

export const fxWriteConformance = () =>
  conformance({
    file: 'fx-writes.test.ts',
    suiteAdapters: fxAdapters,
    inputs: [fxWriteInput],
    fixed: true,
  });

// FX assembles a borrowed DSP source with the include directories of its owner.
// The machine rig uses the directories of the selected manifest, even for borrowed sources.
const sourceOwner = (workspaceRoot, source) => {
  for (let dir = dirname(source); dir !== dirname(dir); dir = dirname(dir))
    if (existsSync(join(workspaceRoot, dir, 'module.toml'))) return dir;
  return null;
};

const moduleInputs = (workspaceRoot, file, root, kind, sources) => {
  const sharcIncludes = read(workspaceRoot, PINS).include.sharc;
  return [
    ...new Set([
      `{workspaceRoot}/${file}`,
      `{workspaceRoot}/${root}/build/gen/gallery.json`,
      ...sources.flatMap((source) => {
        const path = normalize(join(root, source));
        const owner = kind === 'fx' ? sourceOwner(workspaceRoot, path) : root;
        const includes = [...(owner ? [`${owner}/build/gen`, `${owner}/src`] : []), ...sharcIncludes, dirname(path)];
        return [`{workspaceRoot}/${path}`, ...sourceIncludeInputs(workspaceRoot, [path], includes)];
      }),
    ]),
  ];
};

export const moduleRigConformance = (workspaceRoot, file, manifest) => {
  if (!CONFORMANCE_ROOTS.some((root) => file.startsWith(root + '/'))) return;
  const kind = conformanceKind(manifest);
  if (!kind) return;
  const root = dirname(file);
  // FX specs union the declared SHARC sources across the rig devices, independent of packaging ports.
  const devices = packDevices(kind === 'machine' ? manifest.module : {});
  const sources = [...new Set(devices.flatMap((device) => dspSourcePaths(manifest, device)))];
  if (!sources.length) return;
  const variants = negativeVariantsOf(manifest.module.id).map((id) => NEGATIVE_VARIANTS[id][0]);
  const soundInputs = moduleInputs(workspaceRoot, file, root, kind, sources);
  return {
    soundInputs,
    target: conformance({
      root,
      file: `${kind}.test.ts`,
      suiteAdapters: kind === 'machine' ? machineAdapters : fxAdapters,
      inputs: variants.length ? moduleInputs(workspaceRoot, file, root, kind, [...sources, ...variants]) : soundInputs,
      reports: `dist/conformance-rig/merge/${encodeURIComponent(manifest.module.id)}`,
      fixed: true,
    }),
  };
};

// The `test-dsp` target of a module: its sound checks (tests/rig/*.toml, tests/*.rig.test.ts) on the SHARC-only rig. No daemon, firmware or
// emulator. The key holds the module's own files (the DSP sources and the tables of the checks), the kit, the two adapters the rig wraps
// the code in, the SDK, the device profiles, the hashes of the two binaries it runs and the hash of the stock DSP section (or "absent":
// the stock-image suites skip without the firmware). The synthetic shadow-replay fixtures of tests/fixtures/dsp are inputs of the stereo proof's
// shadow replay test.
const STEREO_PROOF_ROOT = 'tests/fixtures/modules/dsp-stereo-proof';

export const rigTarget = (root, moduleInputs) => ({
  executor: 'nx:run-commands',
  cache: true,
  dependsOn: ['patchkit:cli', 'sharc-rig:build'],
  inputs: [
    'jsToolchain',
    ...(moduleInputs ?? ['{projectRoot}/**/*']),
    '{projectRoot}/tests/**/*',
    'dspTestTooling',
    '{workspaceRoot}/packages/sharc-rig-kit/package.json',
    '{workspaceRoot}/vitest.host.config.ts',
    '{workspaceRoot}/vitest.host.projects.json',
    ...machineAdapters,
    ...fxAdapters,
    ...(root === STEREO_PROOF_ROOT ? ['{workspaceRoot}/tests/fixtures/dsp/stereo-*shadow-replay.s'] : []),
    '{workspaceRoot}/sdk/dsp/**/*',
    '{workspaceRoot}/sdk/sdk.toml',
    '{workspaceRoot}/sdk/include/**/*',
    '{workspaceRoot}/devices/*.toml',
    '{workspaceRoot}/tools/patchkit/toolchain/pins.toml',
    { runtime: 'node tools/nx/stock-section-hash.mjs' },
    { dependentTasksOutputFiles: '**/*' },
  ],
  outputs: [],
  options: {
    forwardAllArgs: false,
    command: `pnpm exec vitest run --config vitest.host.config.ts --project module-rig ${root}/tests/`,
    cwd: '.',
  },
});

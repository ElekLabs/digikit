// The shape of device-test targets: dependsOn, command and env, the dt2/dn2 halves and the per-file targets, and the
// conformance, test-only and regression nodes. The cache keys are in device-test-keys.mjs; build and pack targets are in module-build.mjs.
import { globSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { noDspShorts } from './device-files.mjs';
import { checkAcceptanceTags } from './acceptance-tags.mjs';
import { isUploadable, mergeTests, manifestClosure, read, records } from './module-manifests.mjs';
import {
  DEVICE_INPUTS,
  VITEST_SETUP,
  serviceIds,
  deviceInputs,
  forDevice,
  serviceLeaves,
  withoutDriver,
} from './device-test-keys.mjs';

export const IMAGE_BUILD = { target: 'build-objects', projects: ['tag:acceptance-image'] };
// Writes the hash of every committed core object without its source hash (tools/nx/device-bytes.mjs).
export const DEVICE_BYTES = 'patchkit:device-bytes';
// Writes the hash of the image, plan and address map that `patchkit apply` makes for each acceptance firmware and patch set
// (tools/nx/device-images.mjs).
export const DEVICE_IMAGES = 'patchkit:device-images';
// Writes the `needs` verdict of the packs in tests/regression/runtime/needs-packs.json (tools/nx/pack-verdicts.mjs).
export const PACK_VERDICTS = 'patchkit:pack-verdicts';
// The system package every harness card carries (tests/regression/runtime/system-package.ts).
export const SYSTEM_PACKAGE = 'core/runtime:package';
// The host assembly tests of the runtime (runtime/core): the seams between the real packed outputs. No device task starts while they fail.
export const TEST_ASSEMBLY = 'core/runtime:test-assembly';
// Tasks every device task depends on: the patchkit binary, the daemons and the runtime assembly tests. Their projects are the
// shared inputs of every device test (`test:subset --base` cannot select by path for them).
export const SHARED_TASKS = ['patchkit:cli', 'digi-control:daemons', TEST_ASSEMBLY];
// The daemon of the devices without a SHARC DSP (cargo.mjs `daemon-nodsp`, dist/bin/digi-emud-nodsp).
export const DAEMON_NODSP = 'digi-control:daemon-nodsp';
const NO_DSP = noDspShorts();
export const SHARED_PROJECTS = SHARED_TASKS.map((task) => task.split(':')[0]);
// Device and conformance tasks run behind this wrapper: it takes one machine-wide emulator slot. It only schedules and is
// no input: it cannot change a result, and Nx does not cache failures.
export const EMU_SLOT = 'node tools/devtools/emu-slot.mjs --';

// The DSP mode of a run target: coupled when a record that lists one of its files says `"dsp": "sharc"`; no field is zero. emu-slot.mjs takes the cores of the
// task from it (DIGI_EMU_DSP). A test that needs the DSP and runs in a zero record fails in `resolveDsp` (packages/test-support/src/session.ts).
export const taskMode = (tests, recs) => {
  for (const rec of recs)
    if (rec.dsp !== undefined && !['zero', 'sharc'].includes(rec.dsp))
      throw new Error(`metadata.runtimeAcceptance: dsp ${JSON.stringify(rec.dsp)} is not zero or sharc`);
  return recs.some((rec) => rec.dsp === 'sharc' && (rec.files ?? []).some((f) => tests.includes(f))) ? 'sharc' : 'zero';
};

// `opts`: see deviceInputs (device-test-keys.mjs).
export const deviceTarget = (
  root,
  tests,
  recs,
  imageInput,
  importInputs,
  pythonInputs,
  uploadable = false,
  opts = {},
) => {
  const dependsOn = [...SHARED_TASKS, IMAGE_BUILD, DEVICE_BYTES, DEVICE_IMAGES, PACK_VERDICTS, SYSTEM_PACKAGE];
  if (uploadable) dependsOn.push('pack');
  for (const rec of recs) {
    for (const id of rec.modules ?? []) {
      dependsOn.push(`${id}:pack`);
      // a service: the port file that goes on the card with it; the test fixture service takes the port file of the test image (it lists the fixture's hook, which a
      // production port file does not)
      const portTask = id === 'test/svc-test' ? 'patchkit:ports-test' : 'patchkit:ports';
      if (opts.serviceIds?.has(id) && !dependsOn.includes(portTask)) dependsOn.push(portTask);
    }
    if (Object.keys(rec.aliases ?? {}).length) dependsOn.push('pack-aliases');
    if (rec.plusdrive) dependsOn.push('plusdrive-format:cli');
    if (rec.dksampleWasm) dependsOn.push('dksample:wasm');
    if (rec.dkprofWasm) dependsOn.push('dkprof:wasm');
  }
  const { inputs, patchkitLive, patchkitWasm } = deviceInputs(
    root,
    tests,
    recs,
    imageInput,
    importInputs,
    pythonInputs,
    uploadable,
    opts,
  );
  if (patchkitWasm) dependsOn.push('patchkit:wasm'); // the test runs `wb transfer` (patchkit.wasm)
  return {
    executor: 'nx:run-commands',
    cache: true,
    inputs,
    dependsOn: [...new Set(dependsOn)],
    outputs: [],
    options: {
      command: `${EMU_SLOT} pnpm --filter @digi/regression test:runtime ${tests.map((t) => `../../${root}/${t}`).join(' ')}`,
      cwd: '.',
      // a full key holds the patchkit binary, a live closure the patchkit key: runtime/patchkit-exec.ts runs patchkit only then
      env: {
        DIGI_PATCHKIT_LIVE: (!opts.narrow && !opts.merge) || patchkitLive ? '1' : '',
        // the DSP mode of the files (the records' `dsp`): emu-slot.mjs takes the cores of the task from it
        DIGI_EMU_DSP: taskMode(tests, recs),
        // the `-all` twins run the full tier: `full: true` tests (tests/regression/runtime/devices.ts FULL_TIER) run too
        ...(opts.fullTier ? { DIGIKIT_TIER: 'full' } : {}),
      },
    },
  };
};

// One cached target per test file, so an edit or a failure in one file reruns only that file.
// `name` stays the target everything selects: a no-op parent of the per-file targets.
// With `opts.twin`, `name` is the merge target (narrow keys for files outside `opts.mergeTests`) and `${name}-all`
// runs every file with full inputs and DIGIKIT_TIER=full (the gate's full tier).
// Each run target is split in halves, `<name>--dt2` and `<name>--dn2` (env DIGI_DEVICE), so they run in
// parallel. A file gets only the halves of the devices it runs on (`fileDevices`): a half of another device would queue for
// an emulator slot and run nothing. `<name>` stays as a no-op parent with the inputs of the run target.
export const deviceTargets = (
  name,
  root,
  tests,
  recs,
  imageInput,
  importInputs,
  pythonInputs,
  uploadable = false,
  opts = {},
) => {
  const targets = fileTargets(name, root, tests, recs, imageInput, importInputs, pythonInputs, uploadable, opts);
  const devicesOf = (key) => {
    // a run target of one file: the file is the last path in its command
    const files = [...(targets[key].options.command.match(/\S+\.device\.test\.ts/g) ?? [])].map((f) =>
      f.slice(`../../${root}/`.length),
    );
    return unionDevices(files.map((file) => fileDevices(file, recs, opts.projectDevices)));
  };
  const split = {};
  for (const [key, target] of Object.entries(targets)) {
    if (!target.options?.command) {
      split[key] = target;
      continue;
    }
    const devices = devicesOf(key);
    const halves = devices.map((device) => `${key}--${device}`);
    split[key] = { executor: 'nx:noop', inputs: forDevice(target.inputs, null), dependsOn: halves };
    devices.forEach((device, i) => {
      split[halves[i]] = {
        ...target,
        // a device outside the acceptance devices has no daemon key, image hashes or patchkit key in dist/bytes, so a cached result could not follow its inputs
        ...(ALL_DEVICES.includes(device) ? {} : { cache: false }),
        // a device without a SHARC DSP has no entry in dist/daemons.json: its daemon is the build without a core
        ...(NO_DSP.includes(device) ? { dependsOn: [...target.dependsOn, DAEMON_NODSP] } : {}),
        inputs: [...forDevice(target.inputs, device), { env: 'DIGI_DEVICE' }],
        options: { ...target.options, env: { ...target.options.env, DIGI_DEVICE: device } },
      };
    });
  }
  return split;
};

// What a file without `devices` runs on: the devices with a ready snapshot. A device that boots cold (`st`) is named by the file.
const ALL_DEVICES = ['dt2', 'dn2'];
const KNOWN_DEVICES = ['dt2', 'dn2', 'st'];
const unionDevices = (lists) => KNOWN_DEVICES.filter((d) => lists.some((list) => list.includes(d)));

// The devices a device test file runs on (docs/TESTING.md "Nx gate"): the `devices` of the runtimeAcceptance records that
// list the file, else `projectDevices` (the pack devices of a module project), else both. A file that the records do not list
// runs on `projectDevices` or both. tools/nx/tests/device-pins.test.ts checks the declarations against the test sources.
export const fileDevices = (file, recs, projectDevices) => {
  const own = recs.filter((rec) => rec.files?.includes(file));
  const declared = own.filter((rec) => rec.devices !== undefined);
  for (const rec of declared)
    if (!Array.isArray(rec.devices) || !rec.devices.length || rec.devices.some((d) => !KNOWN_DEVICES.includes(d)))
      throw new Error(`metadata.runtimeAcceptance devices of ${file} must be a list of ${KNOWN_DEVICES.join(', ')}`);
  const list = declared.length ? unionDevices(declared.map((rec) => rec.devices)) : (projectDevices ?? ALL_DEVICES);
  if (!list.length) throw new Error(`${file} has no device to run on`);
  return list;
};

const fileTargets = (
  name,
  root,
  tests,
  recs,
  imageInput,
  importInputs,
  pythonInputs,
  uploadable = false,
  opts = {},
) => {
  const allName = `${name}-all`;
  const merge = opts.mergeTests ?? [];
  const isMerge = (test) => !opts.twin || merge.includes(test);
  // `merge`: the key of the merge target (the target everything selects); the `-all` twins keep the full key.
  const build = (list, narrow, recList = recs, merge = false, fullTier = false, siblingTests = []) =>
    deviceTarget(root, list, recList, imageInput, importInputs, pythonInputs, uploadable, {
      ...opts,
      narrow,
      merge,
      fullTier,
      siblingTests,
    });
  if (tests.length === 1) {
    const [test] = tests;
    const targets = { [name]: build(tests, !isMerge(test), recs, Boolean(opts.twin)) };
    if (opts.twin) targets[allName] = build(tests, false, recs, false, true); // the full key for every test, a merge test included
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
    const make = (isNarrow, merge = false, fullTier = false) =>
      build(
        [test],
        isNarrow,
        own.length ? own : recs,
        merge,
        fullTier,
        tests.filter((t) => t !== test),
      );
    targets[child] = make(narrow, Boolean(opts.twin));
    if (opts.twin) all[`${allName}--${suffix}`] = make(false, false, true);
  }
  // Nx selects a target by its inputs, so the parents keep the inputs of the whole project: the merge parent the merge key
  // (without the driver packages), the `-all` parent the full key.
  const { inputs } = build(tests, false);
  const mergeInputs = opts.twin ? build(tests, false, recs, true).inputs : inputs;
  const children = Object.keys(targets);
  if (opts.twin) {
    for (const [key, value] of Object.entries(all)) if (typeof value !== 'string') targets[key] = value;
    targets[allName] = { executor: 'nx:noop', inputs, dependsOn: Object.keys(all) };
  }
  targets[name] = { executor: 'nx:noop', inputs: mergeInputs, dependsOn: children };
  return targets;
};

// A marker that names a missing file would silently give that file a narrow key.
export const checkMergeTests = (workspaceRoot, root, deviceTests, fullTier) => {
  const listed = mergeTests(join(workspaceRoot, root));
  if (!Array.isArray(listed) || listed.some((t) => typeof t !== 'string'))
    throw new Error(`metadata.mergeTests of ${root} must be an array of strings`);
  if (!listed.length) return;
  if (fullTier) throw new Error(`metadata.mergeTests of ${root} has no meaning on a tier:full project`);
  const unknown = listed.filter((t) => !deviceTests.includes(t));
  if (unknown.length)
    throw new Error(`metadata.mergeTests of ${root} names files that are not its device tests: ${unknown.join(', ')}`);
};

// The shared conformance suite loads every uploadable module (`isUploadable` in module-manifests.mjs).
// `full`: the `-all` twin keeps the driver packages in its key; the merge target leaves them out.
export const conformanceTarget = (workspaceRoot, root, m, f, importInputs, full = false) => {
  // the run loads the packs of every uploadable module and runs patchkit live (DIGI_PATCHKIT_LIVE): the merge key follows the
  // packs, the image hashes, the daemon key and the patchkit key of the devices, not the binaries (`-all`: the full key).
  // Its files: the conformance tests and the manifest it reads, not the module's sources.
  const { inputs } = deviceInputs(root, [], [{ sources: [f] }], undefined, importInputs, () => [], false, {
    merge: !full,
    patchkitLive: true,
    own: [],
    base: workspaceRoot,
    testPaths: globSync('tests/conformance/**/*.device.test.ts', { cwd: workspaceRoot }),
  });
  return {
    executor: 'nx:run-commands',
    cache: true,
    inputs: [...forDevice(inputs, null), { env: 'DIGI_DEVICE' }],
    // DEVICE_IMAGES and patchkit:cli write the image hashes and the patchkit key that the merge key holds
    dependsOn: [
      'pack',
      'digikit/debug:pack',
      'digi-control:daemons',
      'patchkit:cli',
      IMAGE_BUILD,
      DEVICE_BYTES,
      DEVICE_IMAGES,
      TEST_ASSEMBLY,
      SYSTEM_PACKAGE,
    ],
    outputs: [],
    options: {
      command: `${EMU_SLOT} pnpm --filter @digi/regression test:conformance`,
      env: {
        DIGI_CONFORMANCE_MODULES: m.id,
        DIGI_CONFORMANCE_MANIFEST: f,
        DIGI_PATCHKIT_LIVE: '1',
        // as tests/conformance/manifests.ts: a dynamic module that is no service runs with the SHARC
        DIGI_EMU_DSP: m.dynamic === true && m.type !== 'service' ? 'sharc' : 'zero',
      },
      cwd: '.',
    },
  };
};

// Nodes of the test-only projects (their device targets); appended to `nodes`.
export const testOnlyNodes = (
  workspaceRoot,
  testOwners,
  byId,
  moduleInfo,
  coreBuilds,
  importInputs,
  pythonInputs,
  nodes,
  imageModules,
) => {
  for (const [file, config] of testOwners) {
    const root = dirname(file);
    const tests = globSync('**/*.device.test.ts', { cwd: join(workspaceRoot, root) })
      .filter(
        (t) =>
          !t.endsWith('.manual.device.test.ts') &&
          !testOwners.some(([pf]) => dirname(pf).startsWith(root + '/') && join(root, t).startsWith(dirname(pf) + '/')),
      )
      .sort();
    checkMergeTests(workspaceRoot, root, tests, config.tags.includes('tier:full'));
    if (!tests.length) continue;
    const recs = records(join(workspaceRoot, root));
    checkAcceptanceTags(root, recs, tests, moduleInfo);
    if (recs.some((rec) => Object.keys(rec.aliases ?? {}).length))
      throw new Error(`Test-only project ${config.name} needs explicit pack-aliases support`);
    // Explicit fixture images build only their manifest requirements; ordinary owners keep the default image.
    const patchRoots = [...new Set(recs.flatMap((rec) => rec.patchModules ?? []))];
    const patchIds = patchRoots.length
      ? manifestClosure(byId, patchRoots).map((file) => read(workspaceRoot, file).module.id)
      : undefined;
    const targetName = config.tags.includes('tier:full') ? 'test-device-full' : 'test-device';
    if (targetName === 'test-device' && !config.implicitDependencies?.length && !patchRoots.length)
      throw new Error(
        `Test-only project ${config.name}: the narrow merge key needs the subject module; name it in implicitDependencies`,
      );
    // a module that goes on the card as a pack: a service (`role`) or an uploadable module
    const services = serviceIds(byId);
    const packed = (id, manifest) => isUploadable(manifest) || services.has(id);
    const targets = deviceTargets(
      targetName,
      root,
      tests,
      recs,
      patchIds ? { input: 'production', projects: patchIds } : null,
      importInputs,
      pythonInputs,
      false,
      {
        twin: targetName === 'test-device',
        subjects: config.implicitDependencies,
        // subjects that are built to objects: their root, to leave the built sources out of the key. Their bytes reach the run
        // through the image hashes (`devices`: the devices whose image holds the subject) or, for a subject that is packed
        // (`pack`: a service or content module), through its pack output. A subject with neither keeps `production`.
        subjectRoots: Object.fromEntries(
          (config.implicitDependencies ?? [])
            .filter((id) => byId.has(id))
            .map((id) => [id, { manifest: byId.get(id).manifest, devices: [...(imageModules.get(id) ?? [])].sort() }])
            .filter(([id, { manifest, devices }]) => (coreBuilds.has(id) && devices.length) || packed(id, manifest))
            .map(([id, { manifest, devices }]) => [
              id,
              { root: dirname(byId.get(id).file), devices, pack: packed(id, manifest) },
            ]),
        ),
        services: serviceLeaves(byId),
        serviceIds: services,
        base: workspaceRoot,
        mergeTests: mergeTests(join(workspaceRoot, root)),
      },
    );
    for (const target of Object.values(targets)) {
      if (!target.options) continue; // the no-op parent
      if (patchIds)
        target.dependsOn = target.dependsOn.flatMap((dependency) =>
          dependency === IMAGE_BUILD
            ? patchIds.filter((id) => coreBuilds.has(id)).map((id) => `${id}:build-objects`)
            : [dependency],
        );
      if (targetName === 'test-device-full') target.options.env = { ...target.options.env, DIGIKIT_TIER: 'full' };
    }
    nodes.push([file, { projects: { [root]: { name: config.name, root, tags: config.tags, targets } } }]);
  }
};

// The node that adds the emulator tests to tests/regression.
export const regressionNode = (workspaceRoot, importInputs) => {
  const imports = importInputs([
    ...globSync('tests/regression/emulator/**/*.test.ts', { cwd: workspaceRoot }),
    VITEST_SETUP,
  ]);
  return [
    'tests/regression/project.json',
    {
      projects: {
        'tests/regression': {
          targets: {
            'test-device': {
              inputs: [...DEVICE_INPUTS, '{projectRoot}/project.json', ...withoutDriver(imports)],
            },
            'test-device-all': {
              inputs: [...DEVICE_INPUTS, '{projectRoot}/project.json', ...imports],
            },
          },
        },
      },
    },
  ];
};

// `wb profile calibrate run`: plays a calibration profile (devices/calibration/profiles/) on an emulator or a device and writes one dk.profile/1 per capture.
// The profile, its step files and the runner are the shared code of packages/workbench-core/src/calibration; this file only connects the pieces a command line has:
// the session and the MIDI port of wb.mjs, the transfer code path of `wb transfer install`, and the files.
//
//     wb [--port SUB | --emu [--run-dir DIR]] profile calibrate run --profile NAME|FILE [--out DIR] [--allow-unidentified-firmware]
//
// NAME is a file of devices/calibration/profiles/. The captures go to --out, by default out/runs/calibration/<time>-<profile>-<target>/, as <case>-<n>.profile.json
// with a run.json that lists them. Exit 1 when a case was skipped (the target lacks something it needs) or the run failed. Use: docs/INSTRUMENTATION.md "Calibration runs".
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { parse as parseToml } from 'smol-toml';
import { existsSync, globSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { MidiOut, RuntimeEvents, wallClock } from '../../packages/dk-session/src/index.ts';
import { SPAN_PHASES } from '../../packages/dk-session/src/gen_proto.ts';
import { button, encoder } from '../../packages/test-support/src/keys.ts';
import {
  addModuleCases,
  parseCalCase,
  parseCalProfile,
} from '../../packages/workbench-core/src/calibration/calprofile.ts';
import { runCalibration } from '../../packages/workbench-core/src/calibration/runner.ts';
import { sessionSetup, untilBootStable } from '../../packages/workbench-core/src/calibration/rigsetup.ts';
import { sessionStatsSource } from '../../packages/workbench-core/src/calibration/stats.ts';
import { parseStepFile } from '../../packages/workbench-core/src/calibration/steps.ts';
import { libraryModule, restartDevice } from '../../packages/workbench-core/src/modules/dktransfer.ts';
import { runId } from '../../packages/workbench-core/src/profile/profile-artifact.ts';
import { transferCommand } from './wb_transfer.mjs';

const dirOf = (root) => path.join(root, 'devices/calibration');

/** The profile named by `--profile`: a file path, or a name in devices/calibration/profiles/. */
function profileFile(root, spec, fail) {
  if (!spec) fail('profile calibrate run --profile NAME|FILE [--out DIR] [--allow-unidentified-firmware]');
  // a file only: the profile named `modules` must not take the `modules/` folder of the repository
  if (existsSync(spec) && statSync(spec).isFile() && !spec.includes('..')) return spec;
  const named = path.join(dirOf(root), 'profiles', `${spec}.toml`);
  if (!existsSync(named)) {
    const known = readdirSync(path.join(dirOf(root), 'profiles')).map((f) => f.replace(/\.toml$/, ''));
    fail(`no profile ${spec} (a file, or one of: ${known.join(', ')})`);
  }
  return named;
}

function stepLibrary(root) {
  const lib = {};
  const folder = path.join(dirOf(root), 'steps');
  for (const f of readdirSync(folder).filter((n) => n.endsWith('.toml'))) {
    const s = parseStepFile(readFileSync(path.join(folder, f), 'utf8'), `steps/${f}`);
    lib[s.name] = s;
  }
  return lib;
}

/**
 * The `calibration.toml` a module ships in its own directory (modules/<kind>/<author>/<name>/), for the modules whose `module.toml` lists a port of `device` (the id before the version of the INFO port):
 * a module that is not built for the target has no case on it (the install would refuse it and end the run).
 */
function moduleCases(root, fail, device) {
  const found = [];
  for (const kind of ['machine', 'fx', 'tool']) {
    const cases = globSync(`modules/${kind}/*/*/calibration.toml`, { cwd: root }).sort((a, b) => {
      const leafA = path.basename(path.dirname(a));
      const leafB = path.basename(path.dirname(b));
      return leafA === leafB ? a.localeCompare(b) : leafA < leafB ? -1 : 1;
    });
    for (const file of cases) {
      const f = path.join(root, file);
      const manifest = parseToml(readFileSync(path.join(path.dirname(f), 'module.toml'), 'utf8'));
      if (!(manifest.module?.ports ?? []).some((p) => p.split('-')[0] === device)) continue;
      try {
        found.push(parseCalCase(readFileSync(f, 'utf8'), file));
      } catch (e) {
        fail(e instanceof Error ? e.message : String(e));
      }
    }
  }
  return found;
}

export async function calibrateRun({ opts, link, host, root, fail, devices, emit }) {
  const file = profileFile(root, opts.profile, fail);
  let profile;
  let steps;
  try {
    profile = parseCalProfile(readFileSync(file, 'utf8'), file);
    steps = stepLibrary(root);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
  }
  const { session } = link;
  // the runtime's INFO names the device by its id (the debug module is needed anyway); the stock Ping can fail on the emulator
  const info = await session.info();
  profile = addModuleCases(profile, moduleCases(root, fail, info.port.split('-')[0]));
  const record = devices.get(opts.device ?? info.port);
  if (!record.midi) fail(`the ${record.name} device file has no [midi] table`);
  const emu = Boolean(opts.emu);
  const clock = emu
    ? {
        now: () => session.clock.now(),
        pause: (ms) => session.clock.pause(ms),
        pauseEmulated: (ms) => session.clock.pauseEmulated(ms),
      }
    : wallClock;

  // names of the module spans: the loaded modules by their record index, refreshed after a restart
  const names = new Map();
  const refreshNames = async () => {
    names.clear();
    for (const m of await session.modules()) names.set(m.index, m.name);
  };
  await refreshNames();
  const nameOf = (s) =>
    s.kind === 'module'
      ? (names.get(s.index) ?? `module ${s.index}`)
      : s.kind === 'phase'
        ? (SPAN_PHASES[s.index] ?? `phase ${s.index}`)
        : `poll ${s.index}`;

  const events = new RuntimeEvents(session, clock);
  await events.listen();

  // modules: the files of the library, the install and selection of `wb transfer install`, the restart of the Transfer pane
  const library = new Map();
  // the modules that stay loaded whatever the case selects (the debug module, the remote module: no rows and no types), found by their file in the library
  let keep = [];
  const bootTimeoutMs = emu ? 600_000 : undefined;
  const modules = {
    async install(ids) {
      const info = await session.info();
      keep = (await session.modules()).filter((m) => m.rows === 0 && m.types.length === 0).map((m) => m.name);
      for (const f of await host.library.scan(info.port)) {
        if (!f.path.endsWith('.dkm')) continue;
        const bytes = new Uint8Array(await host.library.read(info.port, f.path));
        const m = await libraryModule(bytes);
        if (m && (ids.includes(m.id) || keep.includes(m.id)))
          library.set(m.id, { path: m.path, sha256: createHash('sha256').update(bytes).digest('hex') });
      }
      const missingIds = ids.filter((id) => !library.has(id));
      if (missingIds.length)
        throw new Error(
          `not built for ${info.port}: ${missingIds.join(', ')} (mise run module:build, or wb transfer install)`,
        );
      const status = await transferCommand({
        host,
        session,
        sub: 'install',
        rest: ids,
        opts: { ...opts, restart: false, reload: false },
        fail,
      });
      if (status !== 0) throw new Error('wb transfer install refused a module');
      return Object.fromEntries(ids.map((id) => [id, library.get(id).sha256]));
    },
    async select(ids) {
      // the debug module's log is what the run waits on: it stays selected with the case's modules
      const stay = keep.filter((id) => library.has(id) && !ids.includes(id));
      await untilBootStable(session, clock);
      await session.select([...ids, ...stay].map((id) => library.get(id).path));
      await restartDevice(session, emit, { bootTimeoutMs, loadedTimeoutMs: bootTimeoutMs ? 120_000 : undefined });
      await events.listen({ fromBoot: true });
      // the runtime's own MOD_LOAD and MOD_DSP events say when every module of the selection is in; a refusal throws ModuleLoadRefused
      await events.untilModulesActive(ids.length + stay.length);
      await refreshNames();
    },
    // content modules: a service has no rows and no types
    async loaded() {
      return (await session.modules()).filter((m) => m.rows > 0 || m.types.length > 0).map((m) => m.name);
    },
  };

  // the remote module drives keys when it is loaded
  let remote = false;
  try {
    remote = (await session.remoteState()).enabled;
  } catch {
    remote = false;
  }
  // the emulator: wait until the controller's USB MIDI input queue is empty (the firmware took the bytes), as tests/regression/runtime/midi.ts does
  const drained = emu
    ? async () => {
        for (let waited = 0; ; waited += 20) {
          const { queued } = await link.call('midi.usb.in', { bytes: '' });
          if (queued === 0) return;
          if (waited >= 1000) throw new Error('USB MIDI input not taken by the firmware');
          await clock.pause(20);
        }
      }
    : undefined;
  const rig = {
    midi: new MidiOut(link.midiSink),
    ...(drained ? { drained } : {}),
    stats: sessionStatsSource(session, nameOf),
    clock,
    events,
    setup: sessionSetup(session, events, clock),
    modules,
    ...(remote
      ? {
          panel: {
            async tap(key, holdMs = 100) {
              const code = button(key);
              await session.remoteKey(code, true);
              await clock.pause(holdMs);
              await session.remoteKey(code, false);
            },
            turn: (enc, by) => session.remoteTurn(encoder(enc), by),
          },
        }
      : {}),
  };

  // the firmware identity both sides share: INFO `fw_id`, the first 8 bytes of the SHA-256 of the stock firmware (16 hex digits), also what the emulator's runtime reports
  const run = { id: runId(emu ? 'emu' : 'dev', new Date()), target: record.id, runtime_version: info.runtime };
  if (info.fw_id) run.firmware_sha256 = info.fw_id;
  if (info.image_id) run.image_id = info.image_id;
  try {
    run.source_revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  } catch {
    // not a git checkout
  }
  const stamp = new Date()
    .toISOString()
    .replace(/\.\d+Z$/, '')
    .replace(/[-:]/g, '')
    .replace('T', '-');
  const out = path.resolve(
    opts.out ?? path.join(root, 'out/runs/calibration', `${stamp}-${profile.name}-${record.id}-${emu ? 'emu' : 'dev'}`),
  );
  mkdirSync(out, { recursive: true });

  const index = [];
  let skipped = 0;
  console.log(`profile ${profile.name} on ${record.id} (${emu ? 'emulator' : 'device'}), captures in ${out}`);
  const writeIndex = (error) =>
    writeFileSync(
      path.join(out, 'run.json'),
      `${JSON.stringify({ profile: profile.name, run, captures: index, ...(error ? { error } : {}) }, null, 2)}\n`,
    );
  // each capture is written as it finishes, so a later failure loses nothing
  const onCapture = (c) => {
    writeFileSync(path.join(out, c.file), `${JSON.stringify(c.profile)}\n`);
    const p = c.profile;
    const rows = (p.dsp_rows ?? []).filter((x) => x.calls > 0).length;
    const spanTicks = (p.spans ?? []).reduce((a, s) => a + s.sum_ticks, 0);
    console.log(
      `  ${c.file}: ${p.dsp ? `${p.dsp.blocks} DSP blocks, ${rows} rows with calls, stock delta ${p.dsp.max_stock_block_delta}` : 'no DSP stats'}; ${(p.spans ?? []).length} spans, ${spanTicks} ticks`,
    );
    index.push({ case: c.caseId, repeat: c.repeat, file: c.file, input_sha256: p.run.input_sha256 });
    writeIndex();
  };
  const onCase = (r) => {
    if (!r.skipped && !r.failed) return;
    skipped++;
    console.log(`  ${r.id}: ${r.skipped ? `SKIPPED, ${r.skipped}` : `FAILED, ${r.failed}`}`);
    index.push({ case: r.id, ...(r.skipped ? { skipped: r.skipped } : { failed: r.failed }) });
    writeIndex();
  };
  try {
    await runCalibration({
      profile,
      steps,
      rig,
      midi: record.midi,
      run,
      dspUnit: emu ? 'insn' : 'cycles',
      allowUnidentifiedFirmware: Boolean(opts['allow-unidentified-firmware']),
      onCapture,
      onCase,
      log: (line) => console.error(`calibrate: ${line}`),
    });
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    writeIndex(message);
    console.error(`wb: the run failed: ${message} (${index.filter((x) => x.file).length} captures kept in ${out})`);
    return 1;
  }
  writeIndex();
  return skipped > 0 ? 1 : 0;
}

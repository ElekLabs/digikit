// One calibration run of the Profile pane: the rig of a session (MIDI, counters, clock, log events, modules), the shared runner
// (calibration/runner.ts, the code `wb profile calibrate run` plays) and a Stop that ends the run at its next wait and lets the runner's teardown play.
// No module state: the session, the MIDI sink and the modules are arguments, so a test gives fakes. Host tests: tests/calibration-run.test.ts.
import { MidiOut, RuntimeEvents, wallClock, type Clock, type DkSession, type MidiSink } from '@digi/dk-session';
import { SPAN_PHASES } from '@digi/dk-session/gen_proto';
import { deviceOfInfoPort, registry } from '../data/device-profiles.ts';
import { restartDevice } from '../modules/dktransfer.ts';
import {
  CalibrationRunError,
  runCalibration,
  type Capture,
  type CaseResult,
  type Rig,
  type RigModules,
} from '../calibration/runner.ts';
import { captureFirmwareIdentity, selectCalCases } from '../calibration/calprofile.ts';
import { isContentModule, moduleDispatch, sessionEffects } from '../calibration/effects.ts';
import { sessionSetup, untilBootStable } from '../calibration/rigsetup.ts';
import { sessionStatsSource } from '../calibration/stats.ts';
import { resolveRef, type Op } from '../calibration/steps.ts';
import { runId } from './profile-artifact.ts';
import { parseRunLine, type CalFiles, type RunEvent } from './calibration-view.ts';

/** The module files of the library for the target: what the runner installs and selects. */
export interface RunModules {
  library(): Promise<{ id: string; path: string; bytes: Uint8Array }[]>;
  /** Put the modules on the target (the Transfer install). */
  install(ids: string[]): Promise<void>;
}

export interface RunDeps {
  session: DkSession;
  /** The target is the emulator: its clock, its MIDI input drain. */
  emulator: boolean;
  sink: MidiSink;
  /** The emulator's controller call (`midi.usb.in`), for the wait until the firmware has taken the MIDI bytes. */
  call?: <T>(method: string, params?: Record<string, unknown>) => Promise<T>;
  files: CalFiles;
  modules?: RunModules;
  now?: () => Date;
}

export interface RunOptions {
  /** The profile's name (one of `deps.files.profiles`). */
  profile: string;
  cases?: string[];
  trace?: boolean;
  /** Stop: the run ends at its next wait, the teardown of the case in progress plays, the captures made are kept. */
  signal?: AbortSignal;
  onEvent?: (e: RunEvent) => void;
  /** Called once with the run's clock (emulated time on the emulator), for the time left. */
  onClock?: (now: () => Promise<number>) => void;
  onCapture?: (c: Capture) => void;
  onCase?: (r: CaseResult) => void;
}

export interface RunOutcome {
  profile: string;
  emulator: boolean;
  captures: Capture[];
  skipped: { case: string; reason: string }[];
  /** Cases that failed (a capture with no call of any module row); the run went on with the next case. */
  failed: { case: string; reason: string }[];
  /** The run was stopped by the user. */
  stopped: boolean;
  /** Why the run failed (the captures made before are in `captures`). */
  error?: string;
}

class Stopped extends Error {
  constructor() {
    super('stopped');
  }
}

const hex = (d: ArrayBuffer): string => Array.from(new Uint8Array(d), (b) => b.toString(16).padStart(2, '0')).join('');

/** The device a session reports: the id of its INFO port (`<id>-<version>`), which must be in the registry and have a `[midi]` table. */
export function deviceOfInfo(port: string): string {
  const id = deviceOfInfoPort(port);
  if (id === undefined) throw new Error(`the target reports port ${port}, which is no known device`);
  return id;
}

export async function runCalibrationOnTarget(deps: RunDeps, opts: RunOptions): Promise<RunOutcome> {
  const declared = deps.files.profiles.find((p) => p.name === opts.profile);
  if (!declared) throw new Error(`no calibration profile ${opts.profile}`);
  const whole = selectCalCases(declared, opts.cases);
  const { session, emulator } = deps;
  const info = await session.info();
  // A module case whose module has no build for the target (the library holds the builds of this target's device only) is skipped with that reason, and the rest runs.
  const notBuilt: { case: string; reason: string }[] = [];
  let profile = whole;
  if (deps.modules) {
    const built = new Set((await deps.modules.library()).map((f) => f.id));
    const keepCases = whole.cases.filter((c) => {
      const missing = c.modules.filter((id) => !built.has(id));
      if (missing.length === 0) return true;
      notBuilt.push({ case: c.id, reason: `not built for ${info.port}: ${missing.join(', ')}` });
      return false;
    });
    if (notBuilt.length) profile = { ...whole, cases: keepCases };
  }
  const device = deviceOfInfo(info.port);
  const record = registry.get(device);
  if (!record.midi) throw new Error(`the ${record.name} device file has no [midi] table`);

  const base: Clock = emulator ? session.clock : wallClock;
  const signal = opts.signal;
  let tripped = false;
  const aborted = new Promise<void>((resolve) => {
    if (signal?.aborted) resolve();
    else signal?.addEventListener('abort', () => resolve(), { once: true });
  });
  // The first wait after Stop throws, which ends the case (its teardown still plays); later waits return at once so the teardown is not held up.
  const stoppable = async (wait: Promise<void>): Promise<void> => {
    if (tripped) return;
    if (!signal?.aborted) {
      const done = Symbol();
      const r = await Promise.race([wait.then(() => done), aborted.then(() => 'abort' as const)]);
      if (r === done) return;
    }
    tripped = true;
    throw new Stopped();
  };
  const clock: Clock = {
    ...base,
    pause: (ms) => (tripped ? Promise.resolve() : stoppable(base.pause(ms))),
    ...(base.pauseEmulated
      ? {
          pauseEmulated: (ms: number, hangMs?: number) =>
            tripped ? Promise.resolve() : stoppable(base.pauseEmulated!(ms, hangMs)),
        }
      : {}),
  };

  const names = new Map<number, string>();
  const refreshNames = async () => {
    names.clear();
    for (const m of await session.modules()) names.set(m.index, m.name);
  };
  await refreshNames();
  const nameOf = (s: { kind: 'module' | 'phase' | 'poll'; index: number }) =>
    s.kind === 'module'
      ? (names.get(s.index) ?? `module ${s.index}`)
      : s.kind === 'phase'
        ? (SPAN_PHASES[s.index] ?? `phase ${s.index}`)
        : `poll ${s.index}`;

  let events: RuntimeEvents | undefined;
  try {
    events = new RuntimeEvents(session, clock);
    await events.listen();
  } catch {
    events = undefined; // no debug log here: a case that waits for an event is skipped with its reason
  }

  let modules: RigModules | undefined;
  if (deps.modules) {
    const m = deps.modules;
    const byId = new Map<string, { path: string; sha256: string }>();
    // the modules that stay loaded whatever the case selects (the debug module, the remote module: no rows and no types), found by their file in the library
    let keep: string[] = [];
    modules = {
      async install(ids) {
        const files = await m.library();
        keep = (await session.modules()).filter((x) => !isContentModule(x)).map((x) => x.name);
        for (const f of files)
          if ((ids.includes(f.id) || keep.includes(f.id)) && !byId.has(f.id))
            byId.set(f.id, {
              path: f.path,
              sha256: hex(await crypto.subtle.digest('SHA-256', f.bytes as Uint8Array<ArrayBuffer>)),
            });
        const missing = ids.filter((id) => !byId.has(id));
        if (missing.length)
          throw new Error(
            `not built for ${info.port}: ${missing.join(', ')} (build the modules, or install them with Transfer)`,
          );
        await m.install(ids);
        return Object.fromEntries(ids.map((id) => [id, byId.get(id)!.sha256]));
      },
      async select(ids) {
        // the debug module's log is what the run waits on: it stays selected with the case's modules
        const stay = keep.filter((id) => byId.has(id) && !ids.includes(id));
        await untilBootStable(session, clock, (waited, of) => opts.onEvent?.({ kind: 'stable', waited, of }));
        await session.select([...ids, ...stay].map((id) => byId.get(id)!.path));
        await restartDevice(session, () => {}, {});
        if (events) {
          await events.listen({ fromBoot: true });
          // the runtime's own MOD_LOAD and MOD_DSP events say when every module of the selection is in; a refusal throws ModuleLoadRefused
          await events.untilModulesActive(ids.length + stay.length);
        }
        await refreshNames();
      },
      // content modules: a service has no rows and no types
      async loaded() {
        return (await session.modules()).filter(isContentModule).map((x) => x.name);
      },
    };
  }

  const call = deps.call;
  const drained =
    emulator && call
      ? async () => {
          for (let waited = 0; ; waited += 20) {
            const { queued } = await call<{ queued: number }>('midi.usb.in', { bytes: '' });
            if (queued === 0) return;
            if (waited >= 1000) throw new Error('USB MIDI input not taken by the firmware');
            await clock.pause(20);
          }
        }
      : undefined;

  // SETUP (the debug module): a track's machine or filter type through the firmware's own setter, awaited on its SETUP_DONE
  const setup = events ? sessionSetup(session, events, clock) : undefined;

  const rig: Rig = {
    midi: new MidiOut(deps.sink),
    stats: sessionStatsSource(session, nameOf, opts.trace),
    effect: sessionEffects(session),
    dispatch: async (ids) => moduleDispatch(await session.modules(), ids),
    clock,
    ...(events ? { events } : {}),
    ...(setup ? { setup } : {}),
    ...(modules ? { modules } : {}),
    ...(drained ? { drained } : {}),
  };

  const run: Record<string, unknown> & { id: string; target: string } = {
    id: runId(emulator ? 'emu' : 'dev', deps.now?.() ?? new Date()),
    target: device,
    runtime_version: info.runtime,
  };
  if (info.fw_id) Object.assign(run, captureFirmwareIdentity(info.fw_id, record.firmware));
  if (info.image_id) run.image_id = info.image_id;

  // The cases this rig cannot play, as the runner will decide (it has no panel and no screenshots, machine/filter SETUP needs the debug log; waits need what the rig has).
  const cannot = (ops: Op[], mods: string[]): boolean =>
    mods.length > 0 && !modules
      ? true
      : ops.some(
          (o) =>
            (o.op === 'setup' && o.what !== 'effect' && !setup) ||
            o.op === 'key' ||
            o.op === 'turn' ||
            o.op === 'shot' ||
            (o.op === 'wait' && !!o.event && !events) ||
            (o.op === 'wait' && !!o.drained && !drained),
        );
  try {
    const ctx = (where: string) => ({ midi: record.midi, where });
    const around = [profile.setup, profile.teardown].flatMap((r) =>
      r ? resolveRef(r, deps.files.steps, ctx('plan')) : [],
    );
    const skipping = profile.cases
      .filter((c) =>
        cannot(
          [
            ...around,
            ...resolveRef(c.steps, deps.files.steps, ctx(c.id)),
            ...(c.teardown ? resolveRef(c.teardown, deps.files.steps, ctx(c.id)) : []),
          ],
          c.modules,
        ),
      )
      .map((c) => c.id);
    opts.onEvent?.({ kind: 'plan', skipping: [...notBuilt.map((n) => n.case), ...skipping] });
  } catch {
    // a wrong step name: the runner refuses it with its own message
  }
  opts.onClock?.(() => base.now());

  const out: RunOutcome = { profile: profile.name, emulator, captures: [], skipped: [], failed: [], stopped: false };
  for (const n of notBuilt) {
    out.skipped.push(n);
    opts.onEvent?.({ kind: 'skip', case: n.case, reason: n.reason });
    opts.onCase?.({ id: n.case, captures: [], skipped: n.reason });
  }
  try {
    await runCalibration({
      profile,
      steps: deps.files.steps,
      rig,
      midi: record.midi,
      run,
      dspUnit: emulator ? 'insn' : 'cycles',
      onCapture: (c) => {
        out.captures.push(c);
        opts.onCapture?.(c);
      },
      onCase: (r) => {
        if (r.skipped) out.skipped.push({ case: r.id, reason: r.skipped });
        if (r.failed) out.failed.push({ case: r.id, reason: r.failed });
        opts.onCase?.(r);
      },
      log: (line) => opts.onEvent?.(parseRunLine(line)),
    });
  } catch (e) {
    const cause = e instanceof CalibrationRunError ? e.cause : e;
    if (cause instanceof Stopped) out.stopped = true;
    else out.error = e instanceof Error ? e.message : String(e);
  }
  return out;
}

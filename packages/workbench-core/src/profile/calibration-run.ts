// One calibration run of the Profile pane: the rig of a session (MIDI, counters, clock, log events, modules), the shared runner
// (calibration/runner.ts, the code `wb profile calibrate run` plays) and a Stop that ends the run at its next wait and lets the runner's teardown play.
// No module state: the session, the MIDI sink and the modules are arguments, so a test gives fakes. Host tests: tests/calibration-run.test.ts.
import {
  MidiOut,
  RuntimeEvents,
  trackSetup,
  wallClock,
  type Clock,
  type DkSession,
  type MidiSink,
} from '@digi/dk-session';
import { SPAN_PHASES } from '@digi/dk-session/gen_proto';
import { registry } from '../data/device-profiles.ts';
import { restartDevice } from '../modules/dktransfer.ts';
import {
  CalibrationRunError,
  runCalibration,
  type Capture,
  type CaseResult,
  type Rig,
  type RigModules,
} from '../calibration/runner.ts';
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
  /** The run was stopped by the user. */
  stopped: boolean;
  /** Why the run failed (the captures made before are in `captures`). */
  error?: string;
}

/** Clock ms a SETUP request may wait for the debug module's track watch to have its baseline (about 2 s of emulated time after a boot). */
const SETUP_READY_MS = 30_000;

class Stopped extends Error {
  constructor() {
    super('stopped');
  }
}

const hex = (d: ArrayBuffer): string => Array.from(new Uint8Array(d), (b) => b.toString(16).padStart(2, '0')).join('');

/** The device a session reports: the id of its INFO port (`<id>-<version>`), which must be in the registry and have a `[midi]` table. */
export function deviceOfInfo(port: string): string {
  const id = port.split('-')[0] ?? '';
  if (!registry.ids.includes(id)) throw new Error(`the target reports port ${port}, which is no known device`);
  return id;
}

export async function runCalibrationOnTarget(deps: RunDeps, opts: RunOptions): Promise<RunOutcome> {
  const profile = deps.files.profiles.find((p) => p.name === opts.profile);
  if (!profile) throw new Error(`no calibration profile ${opts.profile}`);
  const { session, emulator } = deps;
  const info = await session.info();
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
  const clock: Clock = {
    ...base,
    async pause(ms) {
      if (tripped) return;
      if (!signal?.aborted) {
        const done = Symbol();
        const r = await Promise.race([base.pause(ms).then(() => done), aborted.then(() => 'abort' as const)]);
        if (r === done) return;
      }
      tripped = true;
      throw new Stopped();
    },
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
    modules = {
      async install(ids) {
        const files = await m.library();
        for (const f of files)
          if (ids.includes(f.id) && !byId.has(f.id))
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
        await session.select(ids.map((id) => byId.get(id)!.path));
        await restartDevice(session, () => {}, {});
        if (events) await events.listen({ fromBoot: true });
        await refreshNames();
      },
      // content modules: a service has no rows and no types
      async loaded() {
        return (await session.modules()).filter((x) => x.rows > 0 || x.types.length > 0).map((x) => x.name);
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

  // SETUP (the debug module): a track's machine or filter type through the firmware's own setter, awaited on its SETUP_DONE. A machine given as a module id is
  // the first machine type the runtime reports for that module (the MODULES reply, read when the step plays: the types change with every load).
  const setup: Rig['setup'] = events
    ? async (what, track, type) => {
        if (what === 'effect') throw new Error('SETUP sets machine and filter types, not effects');
        const t = trackSetup(session, events, { ready: { clock, withinMs: SETUP_READY_MS } });
        if (what === 'filter') {
          if (typeof type !== 'number') throw new Error('a filter type is a number');
          await t.setFilter(track - 1, type);
          return;
        }
        let machine: number;
        if (typeof type === 'number') machine = type;
        else {
          const m = (await session.modules()).find((x) => x.name === type);
          if (!m || m.types.length === 0) throw new Error(`module ${type} is not loaded or has no machine type`);
          machine = m.types[0]!;
        }
        await t.setMachine(track - 1, machine);
      }
    : undefined;

  const rig: Rig = {
    midi: new MidiOut(deps.sink),
    stats: sessionStatsSource(session, nameOf),
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
  if (info.fw_id) run.firmware_sha256 = info.fw_id;
  if (info.image_id) run.image_id = info.image_id;

  // The cases this rig cannot play, as the runner will decide (it has no panel and no screenshots, SETUP sets no effects and needs the debug log; waits need what the rig has).
  const cannot = (ops: Op[], mods: string[]): boolean =>
    mods.length > 0 && !modules
      ? true
      : ops.some(
          (o) =>
            (o.op === 'setup' && (o.what === 'effect' || !events)) ||
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
    opts.onEvent?.({ kind: 'plan', skipping });
  } catch {
    // a wrong step name: the runner refuses it with its own message
  }
  opts.onClock?.(() => base.now());

  const out: RunOutcome = { profile: profile.name, emulator, captures: [], skipped: [], stopped: false };
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

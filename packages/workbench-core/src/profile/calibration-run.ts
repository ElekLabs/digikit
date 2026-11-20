// One calibration run of the Profile pane: the rig of a session (MIDI, counters, clock, log events, modules), the shared runner
// (calibration/runner.ts, the code `wb profile calibrate run` plays) and a Stop that ends the run at its next wait and lets the runner's teardown play.
// No module state: the session, the MIDI sink and the modules are arguments, so a test gives fakes. Host tests: tests/calibration-run.test.ts.
import { MidiOut, RuntimeEvents, wallClock, type Clock, type DkSession, type MidiSink } from '@digi/dk-session';
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

class Stopped extends Error {
  constructor() {
    super('stopped');
  }
}

const hex = (d: ArrayBuffer): string => Array.from(new Uint8Array(d), (b) => b.toString(16).padStart(2, '0')).join('');

/** The device a session reports: the id of its INFO port (`dn2-1.12`), which must be in the registry and have a `[midi]` table. */
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
            await base.pause(20);
          }
        }
      : undefined;

  const rig: Rig = {
    midi: new MidiOut(deps.sink),
    stats: sessionStatsSource(session, nameOf),
    clock,
    ...(events ? { events: events as unknown as NonNullable<Rig['events']> } : {}),
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

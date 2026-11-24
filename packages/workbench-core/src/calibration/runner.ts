// The calibration runner: plays a profile's cases on a target (an emulator or a device) and captures DSP stats and runtime spans, one `dk.profile/1`
// per capture. The caller connects and gives the pieces (`Rig`); the runner holds no connection and no module-level state, so `wb profile calibrate run`
// and the workbench's Profile pane use the same code. Host tests: tests/calibration-runner.test.ts. Format: docs/INSTRUMENTATION.md "Calibration runs".
import { ModuleLoadRefused, type Clock, type MidiOut, type RuntimeEvents } from '@digi/dk-session';
import type { MidiMap } from '@digi/devices';
import { buildProfile, dspStatsPart } from '../profile/profile-artifact.ts';
import { canonicalJson, sha256Hex, STOCK_CASE, type CalCase, type CalProfile } from './calprofile.ts';
import { resolveRef, type Op, type ResolveContext, type StepLibrary, type StepRef } from './steps.ts';
import type { StatsReading, StatsSource } from './stats.ts';
import type { DispatchRow, ExpectedEffectRoute } from './effects.ts';

/** The module handling of a target: what the stock case needs (nothing loaded) and the module cases need (exactly their modules). */
export interface RigModules {
  /** Put the modules on the target (an upload); the SHA-256 (hex) of each file by module id. */
  install(ids: string[]): Promise<Record<string, string>>;
  /** Make the content modules (machines and effects, not services) exactly `ids`: select them and restart. Resolves when the target has booted. */
  select(ids: string[]): Promise<void>;
  /** The content modules loaded now (module ids). */
  loaded(): Promise<string[]>;
}

/** What the runner plays on. Pieces a target lacks are left out; a case that needs one is skipped with the reason. */
export interface Rig {
  midi: MidiOut;
  stats: StatsSource;
  /** Emulated time on the emulator, wall time on a device. */
  clock: Clock;
  /** The runtime log: `wait` for an event, and the wait for the DSP sweep that ends a capture. */
  events?: Pick<RuntimeEvents, 'until' | 'listen'>;
  /** Panel keys and encoders (the remote module, or the emulator's panel). */
  panel?: {
    tap(key: string | number, holdMs?: number): Promise<void>;
    turn(encoder: string | number, by: number): Promise<void>;
  };
  /**
   * Machine or filter type through the firmware's setters (the SETUP command of the debug module). `track` is 1 to 16; a machine `type` is a number or the id of
   * the loaded module whose machine it is (`module#n`: its n-th machine, counted from 1). Resolves when the firmware has performed the change.
   */
  setup?: (what: 'machine' | 'filter', track: number, type: number | string) => Promise<void>;
  effect?: (module: string, route: ExpectedEffectRoute) => Promise<DispatchRow>;
  dispatch?: (ids: string[]) => Promise<DispatchRow[]>;
  shot?: (name: string) => Promise<void>;
  /** Returns when the target has taken all queued input. */
  drained?: () => Promise<void>;
  modules?: RigModules;
}

export interface RunInput {
  profile: CalProfile;
  steps: StepLibrary;
  rig: Rig;
  /** The device's `[midi]` table. */
  midi: MidiMap | undefined;
  /** The `run` of every capture: `id`, `target` and what else the caller knows (firmware hash, source revision). `case_id`, `input_sha256` and `module_sha256` are added. */
  run: Record<string, unknown> & { id: string; target: string };
  /** `cycles` on a device, `insn` on the emulator (the unit of the `dsp` header). */
  dspUnit: string;
  /** Called when a capture is complete, before the next one starts: the caller writes it at once, so a later failure loses nothing. */
  onCapture?: (c: Capture) => void | Promise<void>;
  /** Called when a case ended (played or skipped). */
  onCase?: (r: CaseResult) => void | Promise<void>;
  /** Without a `run.firmware_sha256` the captures cannot be refused when device and model differ in firmware: the run is refused unless this is set. */
  allowUnidentifiedFirmware?: boolean;
  log?: (line: string) => void;
}

/** A run that failed: `results` are the cases that finished (their captures were also given to `onCapture`). */
export class CalibrationRunError extends Error {
  readonly results: CaseResult[];
  constructor(message: string, results: CaseResult[], cause: unknown) {
    super(message, { cause });
    this.results = results;
  }
}

export interface Capture {
  caseId: string;
  repeat: number;
  /** `<case>-<repeat>.profile.json`. */
  file: string;
  profile: Record<string, unknown>;
}

export interface CaseResult {
  id: string;
  captures: Capture[];
  /** Why the case did not run (the target lacks something it needs). */
  skipped?: string;
  /** Why the case failed (a module capture without a call of any row): the captures before it are kept, the run goes on with the next case. */
  failed?: string;
}

const isSetup = (o: Op) => o.op === 'setup';
const isEffectSetup = (o: Op): o is Extract<Op, { op: 'setup'; what: 'effect' }> => isSetup(o) && o.what === 'effect';

/** `E_MISSING` of sdk/sdk.toml `[load.status]`: the service the module imports is not in the image. */
const E_MISSING = -73;

/** Clock ms the sweep that ends a capture may take (a sweep is 50 blocks, about 33 ms of the device). */
const SWEEP_WAIT_MS = 5000;
/** Wall ms after which a wait that never ends (a stopped emulator, a dead link) is given up. */
const HANG_GUARD_MS = 120_000;

/** `p`, or an error after `ms` of wall time: the only timer the sweep wait has, a guard against a hang. */
async function withHangGuard<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      p,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${what} (hung for ${ms} ms of wall time)`)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

/** What a case needs that the rig does not have, or undefined. */
function missing(ops: Op[], rig: Rig, modules: string[]): string | undefined {
  for (const o of ops) {
    if (isSetup(o) && o.what === 'effect' && !rig.effect)
      return 'it selects an effect route, which requires the runtime MODULES effect read-back';
    if (isSetup(o) && o.what !== 'effect' && !rig.setup)
      return 'it sets a machine or filter type, which needs the SETUP command of the debug module (the target has no runtime log with it)';
    if ((o.op === 'key' || o.op === 'turn') && !rig.panel)
      return 'it presses keys, and the target has no panel input here';
    if (o.op === 'shot' && !rig.shot) return 'it takes a screenshot, and the target has no screenshot here';
    if (o.op === 'wait' && o.event && !rig.events)
      return 'it waits for a log event, and the target has no runtime log here';
    if (o.op === 'wait' && o.drained && !rig.drained) return 'it waits for drained input, and the target does not tell';
  }
  if (modules.length > 0 && !rig.modules) return 'it loads modules, and the target cannot load them here';
  return undefined;
}

async function play(o: Op, rig: Rig, pause?: (ms: number) => Promise<void>): Promise<void> {
  await playOp(o, rig, pause);
  // a case must not start capturing before the firmware took the notes it was sent
  if (o.op.startsWith('midi.') && rig.drained) await rig.drained();
}

async function playOp(o: Op, rig: Rig, pause?: (ms: number) => Promise<void>): Promise<void> {
  switch (o.op) {
    case 'midi.note':
      return rig.midi.note(o.ch, o.key, o.velocity);
    case 'midi.note_off':
      return rig.midi.noteOff(o.ch, o.key);
    case 'midi.cc':
      return rig.midi.cc(o.ch, o.cc, o.value);
    case 'midi.nrpn':
      return rig.midi.nrpn(o.ch, o.msb, o.lsb, o.value14);
    case 'midi.start':
      return rig.midi.start();
    case 'midi.stop':
      return rig.midi.stop();
    case 'key':
      return rig.panel!.tap(o.key, o.hold_ms);
    case 'turn':
      return rig.panel!.turn(o.encoder, o.by);
    case 'setup':
      if (o.what === 'effect') {
        await rig.effect!(o.type, o.route);
        return;
      }
      return rig.setup!(o.what, o.track, o.type);
    case 'shot':
      return rig.shot!(o.name);
    case 'capture':
      return;
    case 'wait': {
      if (o.ms !== undefined) return (pause ?? ((ms) => rig.clock.pause(ms)))(o.ms);
      if (o.drained) return rig.drained!();
      const match = o.match ?? {};
      await rig.events!.until(o.event as never, (e) => Object.entries(match).every(([k, v]) => e.fields[k] === v), {
        withinMs: o.within_ms ?? 10_000,
      });
    }
  }
}

async function playAll(ops: Op[], rig: Rig, pause?: (ms: number) => Promise<void>): Promise<void> {
  for (const o of ops) await play(o, rig, pause);
}

/** Split a case's operations at its `capture start` and `capture stop` markers: before, the measured window, after. Without markers the window is empty. */
export function splitCapture(ops: Op[], where: string): { pre: Op[]; body: Op[]; post: Op[]; marked: boolean } {
  const at = (mark: 'start' | 'stop') => ops.findIndex((o) => o.op === 'capture' && o.mark === mark);
  const a = at('start');
  const b = at('stop');
  if (a < 0 && b < 0) return { pre: ops, body: [], post: [], marked: false };
  if (a < 0 || b < 0 || b < a)
    throw new Error(`${where}: capture start and capture stop must both be there, start first`);
  if (ops.filter((o) => o.op === 'capture').length !== 2)
    throw new Error(`${where}: one capture start and one capture stop`);
  return { pre: ops.slice(0, a), body: ops.slice(a + 1, b), post: ops.slice(b + 1), marked: true };
}

/** The tracks (1 to 16) the operations act on: SETUP of a machine or filter, and the notes, CCs and NRPNs by their channel (a note off only releases, it uses no track). */
export function opTracks(ops: readonly Op[], midi: MidiMap): number[] {
  const out = new Set<number>();
  for (const o of ops) {
    if (o.op === 'setup' && o.what !== 'effect' && o.track > 0) out.add(o.track);
    else if (o.op === 'midi.note' || o.op === 'midi.cc' || o.op === 'midi.nrpn') {
      const k = midi.trackChannels.indexOf(o.ch);
      if (k >= 0) out.add(k + 1);
    }
  }
  return [...out].sort((a, b) => a - b);
}

/** The tracks a profile uses on a device (its setup, teardown and every case with its own teardown): what a run puts on the stock machine to start. */
export function profileTracks(profile: CalProfile, steps: StepLibrary, midi: MidiMap): number[] {
  const where = (w: string): ResolveContext => ({ midi, where: w });
  const ops = [profile.setup, profile.teardown].flatMap((r) => (r ? resolveRef(r, steps, where('profile')) : []));
  for (const c of profile.cases) {
    ops.push(...resolveRef(c.steps, steps, where(c.id)));
    if (c.teardown) ops.push(...resolveRef(c.teardown, steps, where(c.id)));
  }
  return opTracks(ops, midi);
}

/** Run every case of the profile; the captures are in the result (the caller writes them). A case the target cannot play is skipped with its reason. */
export async function runCalibration(i: RunInput): Promise<CaseResult[]> {
  const { profile, rig } = i;
  const log = i.log ?? (() => {});
  const ctx = (where: string): ResolveContext => ({ midi: i.midi, where });
  // resolve everything first: a wrong name or a track without a channel fails before anything is played
  const setup = profile.setup ? resolveRef(profile.setup, i.steps, ctx('setup')) : [];
  const teardown = profile.teardown ? resolveRef(profile.teardown, i.steps, ctx('teardown')) : [];
  const resolved = profile.cases.map((c) => ({
    c,
    ops: resolveRef(c.steps, i.steps, ctx(`case ${c.id}`)),
    after: c.teardown ? resolveRef(c.teardown, i.steps, ctx(`case ${c.id} teardown`)) : [],
  }));
  for (const { c, ops } of resolved) {
    const { body, post } = splitCapture(ops, `case ${c.id}`);
    if (body.some(isEffectSetup) || post.some(isEffectSetup))
      throw new Error(`case ${c.id}: check the effect route before the capture window`);
  }
  if (!i.run.firmware_sha256 && !i.allowUnidentifiedFirmware)
    throw new Error(
      'the target reported no firmware identity (run.firmware_sha256): a fit could not refuse a device and a model with different firmware; allow it explicitly to capture anyway',
    );
  const caps = await rig.stats.capabilities();
  if (caps.dsp && !rig.events)
    throw new Error(
      'the target has DSP stats, and the runner waits for their DSP_SWEEP event: the rig needs the runtime log',
    );
  if (!caps.dsp && profile.cases.some((c) => c.modules.length > 0))
    throw new Error(
      'the profile has module cases, and the target has no DSP stats (PROFILE_DSP): their captures would carry no DSP rows, so nothing is written',
    );
  if (!caps.dsp) log('DSP stats are not available on this target (no PROFILE_DSP): the captures carry spans only');

  // counters an earlier run left on would keep DSP_SWEEP in the log and count a stale generation: stop them first
  await rig.stats.stop();

  // decide what runs before the target is touched: a skipped case installs and loads nothing
  const plan = resolved.map(({ c, ops, after }) => ({
    c,
    ops,
    after,
    reason: missing([...setup, ...ops, ...teardown, ...after], rig, c.modules),
  }));
  const needed = [...new Set(plan.filter((x) => !x.reason).flatMap((x) => x.c.modules))];
  const results: CaseResult[] = [];
  try {
    // a known start: the tracks the run uses go to the stock machine (type 0, valid in every boot), whatever the project had on them (a run-time machine whose
    // module is not loaded in this boot would be refused later, when it is put back). Nothing is restored at the end: a case leaves the stock machine too.
    const runnable = plan.filter((x) => !x.reason);
    if (rig.setup && i.midi && runnable.length > 0) {
      const tracks = opTracks([...setup, ...teardown, ...runnable.flatMap((x) => [...x.ops, ...x.after])], i.midi);
      for (const t of tracks) {
        try {
          await rig.setup('machine', t, 0);
        } catch (e) {
          throw new Error(
            `the run could not put track ${t} on the stock machine before the first case (${e instanceof Error ? e.message : e}): nothing was loaded or changed after that; check that the debug module is selected and the device is not busy, then run again`,
          );
        }
      }
      if (tracks.length) log(`tracks ${tracks.join(', ')} are on the stock machine`);
    }
    let hashes: Record<string, string> = {};
    if (needed.length > 0) {
      hashes = await rig.modules!.install(needed);
      for (const id of needed) if (!hashes[id]) throw new Error(`module ${id}: no file to install (build it first)`);
    }
    for (const { c, ops, after, reason } of plan) {
      if (reason) {
        log(`case ${c.id}: skipped, ${reason}`);
        const r = { id: c.id, captures: [], skipped: reason };
        results.push(r);
        await i.onCase?.(r);
        continue;
      }
      const r = await runCase(i, c, setup, ops, teardown, after, hashes, caps.dsp, log);
      results.push(r);
      await i.onCase?.(r);
    }
  } catch (e) {
    throw new CalibrationRunError(e instanceof Error ? e.message : String(e), results, e);
  }
  return results;
}

/** Make the loaded content modules exactly `want` (the stock case: none), and check it. */
async function ensureModules(rig: Rig, want: string[], log: (l: string) => void, caseId: string): Promise<void> {
  if (!rig.modules) return;
  const same = (a: string[], b: string[]) =>
    a.length === b.length && [...a].sort().join('\n') === [...b].sort().join('\n');
  let have = await rig.modules.loaded();
  if (!same(have, want)) {
    log(`loading ${want.length === 0 ? 'no modules' : want.join(', ')}`);
    log(`case ${caseId}: restarting`);
    await rig.modules.select(want);
    have = await rig.modules.loaded();
  }
  if (!same(have, want))
    throw new Error(`the target has modules [${have.join(', ')}] loaded, expected [${want.join(', ')}]`);
}

async function runCase(
  i: RunInput,
  c: CalCase,
  setup: Op[],
  ops: Op[],
  teardown: Op[],
  after: Op[],
  hashes: Record<string, string>,
  dsp: boolean,
  log: (l: string) => void,
): Promise<CaseResult> {
  const { profile, rig } = i;
  const { pre, body, post, marked } = splitCapture(ops, `case ${c.id}`);
  const workload = {
    profile: {
      name: profile.name,
      warmup_s: profile.warmup_s,
      capture_s: profile.capture_s,
      repeats: profile.repeats,
    },
    modules: c.modules,
    setup,
    case: ops,
    teardown: [...teardown, ...after],
  };
  const captures: Capture[] = [];
  try {
    await ensureModules(rig, c.modules, log, c.id);
  } catch (e) {
    // the runtime refused the module because the image lacks a core module the case declares it needs: the case cannot run here
    const declared = c.needs ?? [];
    const lacking = e instanceof ModuleLoadRefused ? e.needs : undefined;
    const named = lacking !== undefined && declared.some((n) => n.replace(/^core\//, '') === lacking);
    // E_MISSING: a service the module imports is not in the image (the module that provides it is not part of it); the case says what it needs
    const service = e instanceof ModuleLoadRefused && e.result === E_MISSING && declared.length > 0;
    if (named || service) {
      const reason = named
        ? `the image has no core/${lacking}, which the module needs`
        : `the image lacks a service the module needs (the case declares ${declared.join(', ')})`;
      log(`case ${c.id}: skipped, ${reason}`);
      return { id: c.id, captures: [], skipped: reason };
    }
    throw e;
  }
  // notes with `retrigger_ms` are pressed again during every wait of the capture, so that a voice that ends by itself sounds for the whole window
  const keepers = [...setup, ...pre].filter(
    (o): o is Extract<Op, { op: 'midi.note' }> => o.op === 'midi.note' && o.retrigger_ms !== undefined,
  );
  // warm-up and capture windows hold emulated time (the emulator's `pauseEmulated`), not wall time
  const hold = (ms: number): Promise<void> => (rig.clock.pauseEmulated ?? rig.clock.pause).call(rig.clock, ms);
  const pause = async (ms: number): Promise<void> => {
    if (keepers.length === 0) return hold(ms);
    const every = Math.min(...keepers.map((k) => k.retrigger_ms!));
    let next = every;
    for (let left = ms; left > 0;) {
      const slice = Math.min(next, left);
      await hold(slice);
      left -= slice;
      // pressed again only when the wait goes on: a press at its end would start a voice that the next measurement sees from its first block
      if (left > 0) {
        // sending the key takes clock time too (the drain wait): the next slice is shorter by it, so the period stays `retrigger_ms`
        const before = await rig.clock.now();
        for (const k of keepers) await play(k, rig);
        next = Math.max(1, every - ((await rig.clock.now()) - before));
      } else next = every;
    }
  };
  let failed: string | undefined;
  let stopError: Error | undefined;
  try {
    await playAll(setup, rig);
    await playAll(pre, rig);
    const requests = [...setup, ...pre].filter(isEffectSetup);
    const selected = await Promise.all(requests.map((o) => rig.effect!(o.type, o.route)));
    const rows = c.modules.length ? await rig.dispatch?.(c.modules) : undefined;
    const expected = selected.length
      ? [...(rows ?? []).filter((row) => row.kind === 'machine'), ...selected]
      : rows?.length
        ? rows
        : undefined;
    if (c.modules.length && rig.dispatch && !expected?.length)
      throw new Error(`case ${c.id}: no DSP dispatch rows were published for the selected modules`);
    const input = await sha256Hex(canonicalJson(workload));
    for (let r = 1; r <= profile.repeats; r++) {
      log(`case ${c.id}: capture ${r} of ${profile.repeats}`);
      let stopped = false;
      let reading: StatsReading;
      try {
        await rig.stats.start();
        await pause(profile.warmup_s * 1000);
        const { generation } = await rig.stats.clear();
        if (marked) await playAll(body, rig, pause);
        else await pause(profile.capture_s * 1000);
        await rig.stats.mark?.('Workload end');
        if (dsp) {
          // the window ends with the first complete sweep of this clear generation that the mirror publishes after it (about 50 blocks): the runtime logs DSP_SWEEP
          await rig.events!.listen();
          await withHangGuard(
            rig.events!.until('DSP_SWEEP', (e) => e.fields.generation === generation, { withinMs: SWEEP_WAIT_MS }),
            HANG_GUARD_MS,
            `case ${c.id}: no DSP_SWEEP of generation ${generation}`,
          );
        }
        // the spans and the sweep are read after counting stopped, so they describe one window
        await rig.stats.stop();
        stopped = true;
        reading = await rig.stats.read();
      } finally {
        // counters never keep running after a failure
        if (!stopped) await rig.stats.stop().catch(() => {});
      }
      const dspOut = dspStatsPart(reading.dsp?.sweep, i.dspUnit);
      if (dsp && !dspOut) throw new Error(`case ${c.id}: no complete DSP sweep after the capture`);
      if (dspOut && c.modules.length > 0) {
        const busy = dspOut.rows.filter((x) => x.calls > 0);
        const missing = expected?.filter((wanted) => {
          if (wanted.kind === 'effect') return !busy.some((row) => row.kind === 'effect' && row.row === wanted.row);
          return !expected.some(
            (candidate) =>
              candidate.module === wanted.module &&
              candidate.kind === 'machine' &&
              busy.some((row) => row.kind === 'machine' && row.row === candidate.row),
          );
        });
        if (missing?.length) {
          failed = `capture ${r} did not count the requested DSP rows (${missing.map((row) => row.module + ' row ' + row.row).join(', ')})`;
          log(`case ${c.id}: failed, ${failed}`);
          break;
        }
        if (busy.length === 0) {
          // a failure of this case, recorded; the run goes on with the next case
          failed = `capture ${r} counted no calls of any machine or effect row: nothing of the module sounded in the window (a voice that ends by itself needs retrigger_ms on its note)`;
          log(`case ${c.id}: failed, ${failed}`);
          break;
        }
        for (const x of busy)
          if (x.calls < 0.5 * dspOut.header.blocks)
            log(
              `case ${c.id}: capture ${r}: row ${x.row} has ${x.calls} calls in ${dspOut.header.blocks} blocks: the voice did not sound the whole window`,
            );
      }
      if (c.id === STOCK_CASE && dspOut?.rows.some((x) => x.calls > 0))
        throw new Error(
          `case ${c.id}: the stock case counted module rows (a module is loaded or a module machine plays)`,
        );
      for (const [index, request] of requests.entries()) {
        const current = await rig.effect!(request.type, request.route);
        if (current.row !== selected[index]?.row)
          throw new Error(`case ${c.id}: the effect dispatch row changed during capture`);
      }
      const profileOut = buildProfile({
        run: {
          ...i.run,
          id: `${i.run.id}-${c.id}-${r}`,
          case_id: c.id,
          input_sha256: input,
          workload,
          ...(expected ? { dispatch: expected } : {}),
          module_sha256: c.modules.map((m) => hashes[m]!).sort(),
        },
        spans: reading.spans,
        trace: reading.trace,
        traceError: reading.traceError,
        ...(dspOut ? { dsp: dspOut } : {}),
      });
      const capture = { caseId: c.id, repeat: r, file: `${c.id}-${r}.profile.json`, profile: profileOut };
      captures.push(capture);
      await i.onCapture?.(capture);
    }
    if (!failed) await playAll(post, rig);
  } finally {
    // the notes of the case must not hold on into the next one, also after a failure
    await playAll(teardown, rig).catch((e) =>
      log(`case ${c.id}: teardown failed: ${e instanceof Error ? e.message : e}`),
    );
    // what the case changed on the target (the track's machine type) goes back to the stock machine before the next case loads other modules; a refusal stops the run
    if (after.length > 0) log(`case ${c.id}: putting the stock machine back`);
    await playAll(after, rig).catch((e) => {
      stopError = e instanceof Error ? e : new Error(String(e));
    });
  }
  if (stopError)
    throw new Error(
      `case ${c.id}: putting the track's machine type back failed (${stopError.message}): the run stops before the next case unloads the module, so that no track keeps the machine type of an unloaded module`,
    );
  return { id: c.id, captures, ...(failed ? { failed } : {}) };
}

export type { StepRef };

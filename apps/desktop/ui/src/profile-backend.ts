// What the Profile pane reads and does (ProfilePanel.tsx): the runtime's span counters of the debug module over the target's session (dk-session
// profileState / profileAll, PROFILE_STATE and PROFILE_READ), and on the emulator also `cf.profile` and `sharc.profile` of the controller (the
// Host's `rpc`). One recorder per panel: no module state; the session, the rpc, the library and dkprof.wasm are arguments, so a test gives fakes.
// The preview replaces this module (preview/profile-backend.mock.ts). Host tests: tests/profile-backend.test.ts.
import { latestDeviceHealth } from './device-health';
import { CAP } from '@digi/dk-session/gen_proto';
import { DkError, type DkSession, type PollSlot } from '@digi/dk-session';
import type { DkProf } from '@digi/dk-session/dkprof';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import type { LibraryModule } from '@digi/workbench-core/modules/dktransfer.ts';
import {
  buildProfile,
  cfPart,
  runId,
  sharcPart,
  spansPart,
  type BuildInput,
  type CfAnswer,
  CF_PATHS,
  type Part,
  type SharcAnswer,
} from '@digi/workbench-core/profile/profile-artifact.ts';
import {
  dspView,
  ownerName,
  type DspView,
  type ModuleRef,
  type OwnerKind,
  type PollInfo,
  type Snapshot,
} from '@digi/workbench-core/profile/profile-core.ts';
import { stockImageHash, type LogNames } from '@digi/dk-session/log-names';
import type { FrozenTrace } from './span-recording';
import { sampledPcs, type Hotspot } from './profile-hotspots';

/** The part of the session the pane uses. */
export type ProfileSession = Pick<
  DkSession,
  'info' | 'modules' | 'capacity' | 'profileState' | 'profileAll' | 'profileTrace' | 'logPage' | 'peek'
>;

/** One ColdFire path of the emulator (ram_clear, run_fast, step_once, idle_skip) with its share of the host time and of the instructions. */
export interface CfPathRow {
  path: string;
  hostNs: number;
  insns: number;
  calls: number;
  /** Share of the summed host time and of the summed instructions of all paths, 0..1 (0 when the sum is 0). */
  timeShare: number;
  insnShare: number;
}

/** The compact ColdFire view of the Profile pane. */
export interface CfView {
  paths: CfPathRow[];
  pcs: Hotspot[];
}

export const CF_TOP_PCS = 10;

/** The paths and the heaviest sampled PCs of a `cf.profile` part. Pure: the names come from `nameOf` (address to text), if given. */
export function cfView(part: Part, nameOf?: (addr: number) => string | undefined, top = CF_TOP_PCS): CfView {
  const metric = (path: string, what: string): number =>
    part.metrics.find((m) => m.name === `path.${path}.${what}`)?.value ?? 0;
  const sum = (key: 'hostNs' | 'insns', rows: { hostNs: number; insns: number }[]) =>
    rows.reduce((t, r) => t + r[key], 0);
  const raw = CF_PATHS.map((path) => ({
    path,
    hostNs: metric(path, 'host_ns'),
    insns: metric(path, 'instructions'),
    calls: metric(path, 'calls'),
  }));
  const ns = sum('hostNs', raw);
  const ins = sum('insns', raw);
  const paths = raw.map((r) => ({
    ...r,
    timeShare: ns > 0 ? r.hostNs / ns : 0,
    insnShare: ins > 0 ? r.insns / ins : 0,
  }));
  const pcs = sampledPcs(part.raw, 'cf', top);
  for (const row of pcs) {
    const name = nameOf?.(row.addr);
    if (name) row.name = name;
  }
  return { paths, pcs };
}

/** The whole-block DSP load of a device, from its newest HEALTH event (percent of the block). */
export interface DeviceDsp {
  avgPct: number;
  maxPct: number;
}

/** One reading. `noDebug`: the target runs no debug module (or an older one without PROFILE_STATE): the pane offers to add it. */
export type ProfileReading =
  | { noDebug: true; port: string; emulator: boolean }
  | {
      noDebug: false;
      port: string;
      /** The target is the emulator (the controller's instruction profiles go with the spans). */
      emulator: boolean;
      observedAt: number;
      imageId?: string | null;
      traceSummary?: string;
      frozenTrace?: FrozenTrace;
      traceSupported?: boolean;
      snap: Snapshot;
      modules: ModuleRef[];
      /** Emulator: the modules' DSP per block (undefined when `sharc.profile` is off or the daemon lacks it). */
      dsp?: DspView;
      /** Hardware: the newest HEALTH event, when the ring holds one. */
      deviceDsp?: DeviceDsp;
      /** Emulator: the ColdFire paths and heaviest PCs of `cf.profile` since Start (undefined when it is off or the daemon lacks it). */
      cf?: CfView;
      /** The emulated device has no DSP: the pane has no DSP section. */
      noDsp?: boolean;
      /** What the emulator's two instruction profiles could not give (the daemon lacks the feature), one line. */
      note?: string;
    };

export interface ProfileDeps {
  session: () => Promise<ProfileSession>;
  /** The controller's rpc: set when the target is the emulator (Start then also starts `cf.profile` and `sharc.profile`). */
  rpc?: EmuRpc;
  loadLibrary: (device: string) => Promise<LibraryModule[]>;
  prof?: () => Promise<DkProf>;
  /** The maps and load table of the running image (the emulator's names), to name the DSP counters. */
  names?: Pick<LogNames, 'inputs' | 'refresh' | 'needsRefresh' | 'imageHash'>;
  now?: () => Date;
}

/** The debug module is not there: no PEEK capability, or PROFILE_STATE is an unknown command of an older debug module. */
export const isDebugMissing = (e: unknown): boolean => e instanceof DkError && e.status === 'E_PROTO';

const KINDS: Record<LibraryModule['group'], OwnerKind> = {
  Machines: 'machine',
  Effects: 'effect',
  Filters: 'tool',
  'LFO shapes': 'tool',
  Tools: 'tool',
  Services: 'tool',
};

const asFault = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const METHOD_NOT_FOUND = -32601;

/** The daemon answered "method not found" or "built without ..." (a feature compiled out), not another error. */
export const isBuiltWithout = (e: unknown): boolean =>
  (e as { code?: number } | null)?.code === METHOD_NOT_FOUND || /built without|method not found/i.test(asFault(e));

/** A device file says the device has no DSP; a device the registry does not know is taken to have one. */
export const deviceHasDsp = (device: string): boolean =>
  !registry.ids.includes(device) || registry.get(device).capabilities.dsp === 'sharc';

export const NO_DSP_NOTE = 'no DSP on this device';

/** What the emulator's instruction profiles could not give: methods the daemon lacks, other errors, no DSP. */
interface ProfileFaults {
  without: string[];
  other: string[];
  noDsp?: boolean;
}

function addFault(f: ProfileFaults, method: string, e: unknown): void {
  if (isBuiltWithout(e)) f.without.push(method);
  else f.other.push(`${method}: ${asFault(e)}`);
}

function noteOf(f: ProfileFaults): string | undefined {
  const lines: string[] = [];
  if (f.without.length)
    lines.push(`${f.without.join(' and ')} not available: the emulator daemon was built without it (docs/CONTROL.md).`);
  lines.push(...f.other);
  if (f.noDsp) lines.push(`sharc.profile: ${NO_DSP_NOTE}.`);
  return lines.length ? lines.join(' ') : undefined;
}

/**
 * One source of a recording. Every source starts, stops, resets and reads the same way; a source that cannot run on this target is simply not
 * made (`absent`, with the reason the pane shows). `reset` zeroes the counters and keeps counting.
 */
interface Source<T> {
  start(): Promise<void>;
  stop(): Promise<void>;
  reset(): Promise<void>;
  read(): Promise<T>;
}

export interface Recorder {
  /** Read the state, the spans and the modules (and, on the emulator, the DSP per block). */
  read(detail?: 'full' | 'spans'): Promise<ProfileReading>;
  /** Reset the counters and start the spans; on the emulator also `cf.profile` and `sharc.profile` count from now. */
  start(): Promise<void>;
  /** Stop the spans; on the emulator, keep the instruction profiles of the recording for Export. */
  stop(): Promise<void>;
  /** Zero the counters (both states); the recording window starts again. */
  reset(): Promise<void>;
  /** The `dk.profile/1` of what was recorded (spans, and on the emulator cf.profile and sharc.profile), as the file text and a file name. */
  exportFile(): Promise<{ text: string; name: string }>;
}

type ProfileInfo = Awaited<ReturnType<ProfileSession['info']>>;

const sameImage = (a: ProfileInfo, b: ProfileInfo): boolean =>
  a.port === b.port && a.image_id === b.image_id && a.fw_id === b.fw_id;

export function createRecorder(deps: ProfileDeps): Recorder {
  const now = deps.now ?? (() => new Date());
  let library: { port: string; modules: LibraryModule[] } | undefined;
  let device = 'unknown';
  /** The instruction profiles of the last recording, kept at Stop. */
  let kept: { cf?: Part; sharc?: Part } = {};
  /** The emulator sources that started for the running recording (each one that failed to start is in `instructionNote`). */
  let active: { cf?: Source<Part>; sharc?: Source<Part> } = {};
  const counting = (): boolean => active.cf !== undefined || active.sharc !== undefined;
  let instructionNote: string | undefined;
  /** Per source: how often the daemon started a new runtime during the recording (its counters restart with each). */
  let restarts: { cf?: number; sharc?: number } = {};
  /** What could not start for the running recording; read faults come on top. */
  let startFaults: ProfileFaults = { without: [], other: [] };

  let namesImage: ProfileInfo | undefined;
  let namesRequest = 0;
  let captureImage: { info: ProfileInfo; sha256: string } | undefined;
  let traceGeneration: number | undefined;
  let frozenTrace: FrozenTrace | undefined;
  let stoppedCapture: BuildInput | undefined;

  const nameInputs = (info: ProfileInfo) =>
    namesImage && sameImage(info, namesImage) && !deps.names?.needsRefresh?.() ? deps.names?.inputs() : undefined;

  async function refreshNames(s: ProfileSession, info: ProfileInfo): Promise<void> {
    if (!deps.names) return;
    const request = ++namesRequest;
    namesImage = undefined;
    try {
      await deps.names.refresh(s);
      if (request === namesRequest) namesImage = info;
    } catch {
      // Names are optional; the profile keeps its raw addresses.
    }
  }

  async function captureIdentity(s: ProfileSession, info: ProfileInfo): Promise<void> {
    captureImage = undefined;
    await refreshNames(s, info);
    const ship = nameInputs(info)?.maps[0];
    const sha256 = deps.names?.imageHash?.() ?? (ship && stockImageHash(ship));
    if (sha256 && info.image_id && (!info.fw_id || sha256.startsWith(info.fw_id.toLowerCase()))) {
      const current = await s.info().catch(() => undefined);
      if (current && sameImage(info, current)) captureImage = { info, sha256 };
    }
  }

  const libraryOf = async (port: string): Promise<LibraryModule[]> => {
    if (library?.port !== port) {
      let modules: LibraryModule[] = [];
      try {
        modules = await deps.loadLibrary(port);
      } catch {
        // no library folder or no needs reader: the modules are not told apart
      }
      library = { port, modules };
    }
    return library.modules;
  };

  /** The poll slots with the function names of the address maps (the Log pane's maps and load table); a slot without a name keeps its address only. */
  const pollInfos = async (slots: (PollSlot | undefined)[], info: ProfileInfo): Promise<(PollInfo | undefined)[]> => {
    const polls: (PollInfo | undefined)[] = slots.map((p) => (p ? { owner: p.owner, fn: p.fn } : undefined));
    const request = namesRequest;
    const inputs = nameInputs(info);
    const used = polls.flatMap((p) => (p ? [p] : []));
    if (!deps.prof || !inputs || used.length === 0) return polls;
    try {
      const prof = await deps.prof();
      if (request !== namesRequest || deps.names?.needsRefresh?.()) return polls;
      const { results } = prof.addrLookup({
        maps: inputs.maps,
        loads: inputs.loads,
        addrs: used.map((p) => ({ cpu: 'cf' as const, addr: p.fn })),
      });
      used.forEach((p, i) => {
        const r = results[i];
        if (r?.text) p.name = r.text;
      });
    } catch {
      // no names: the rows say the owner only
    }
    return polls;
  };

  /** The ColdFire view of a `cf.profile` part, its heaviest PCs named through the same lookup as the poll slots (dkprof and the image's maps). */
  const cfViewOf = async (part: Part, info: ProfileInfo): Promise<CfView> => {
    const view = cfView(part);
    const request = namesRequest;
    const inputs = nameInputs(info);
    if (deps.prof && inputs && view.pcs.length > 0) {
      try {
        const prof = await deps.prof();
        if (request !== namesRequest || deps.names?.needsRefresh?.()) return view;
        const { results } = prof.addrLookup({
          maps: inputs.maps,
          loads: inputs.loads,
          addrs: view.pcs.map((p) => ({ cpu: 'cf' as const, addr: p.addr })),
        });
        view.pcs.forEach((p, i) => {
          const t = results[i]?.text;
          if (t) p.name = t;
        });
      } catch {
        // no names: the rows show the address
      }
    }
    return view;
  };

  const moduleRefs = async (s: ProfileSession, port: string): Promise<ModuleRef[]> => {
    const lib = await libraryOf(port);
    return (await s.modules()).map((m) => ({
      index: m.index,
      name: m.name,
      kind: KINDS[lib.find((l) => l.name === m.name)?.group as LibraryModule['group']] ?? 'other',
    }));
  };

  const call = <T>(method: string, params?: Record<string, unknown>): Promise<T> => {
    if (!deps.rpc) return Promise.reject(new Error('no emulator'));
    return deps.rpc.call<T>(method, params);
  };

  const hasDsp = (): boolean => deviceHasDsp(device);

  /** The message for an E_PROTO of PROFILE_STATE: the debug module is loaded (PEEK) but has no span clock, or it is not loaded. */
  const stateFault = (e: unknown, info: { port?: string | null; caps: number | null }): unknown => {
    if (!isDebugMissing(e)) return e;
    if (info.caps === null || ((info.caps >>> CAP.PEEK) & 1) === 0) return new Error('debug module not loaded');
    const stem = registry.ids.includes(device) ? registry.get(device).stem : '<device>';
    return new Error(`no profiling clock on this device (devices/${stem}.toml [digikit] span_clock)`);
  };

  /** PROFILE_STATE with the E_PROTO of a missing debug module or span clock said in words. */
  const state = async (s: ProfileSession, op: Parameters<ProfileSession['profileState']>[0]) => {
    try {
      return await s.profileState(op);
    } catch (e) {
      throw stateFault(e, await s.info());
    }
  };

  /** A controller method with `{enable}`: true starts and clears, false stops and clears, no argument reads the counts since the start. */
  const emuSource = <A, T>(method: string, part: (a: A) => T): Source<T> => ({
    start: async () => void (await call(method, { enable: true })),
    stop: async () => void (await call(method, { enable: false })),
    reset: async () => void (await call(method, { enable: true })),
    read: async () => part(await call<A>(method)),
  });
  const cfSource = (): Source<Part> =>
    emuSource<CfAnswer, Part>('cf.profile', (a) => {
      restarts.cf = a.restarts;
      return cfPart(undefined, a);
    });
  const sharcSource = (): Source<Part> =>
    emuSource<SharcAnswer, Part>('sharc.profile', (a) => {
      restarts.sharc = a.restarts;
      return sharcPart(a);
    });

  const spansSource = (s: ProfileSession): Source<void> => ({
    start: async () => {
      const caps = await state(s, 'reset');
      const supported = caps.traceVersion === 1;
      const started = await state(s, supported ? 'startTrace' : 'start');
      traceGeneration = started.traceGeneration;
    },
    stop: async () => void (await state(s, 'stop')),
    reset: async () => void (await s.profileState('reset')),
    read: async () => {},
  });

  /** The instruction profiles of the emulator since the start; a source that fails to read is said once in `instructionNote`. */
  async function instructionParts(): Promise<{ cf?: Part; sharc?: Part }> {
    const out: { cf?: Part; sharc?: Part } = {};
    const faults: ProfileFaults = { without: [...startFaults.without], other: [...startFaults.other] };
    for (const key of ['cf', 'sharc'] as const) {
      try {
        const src = active[key];
        if (src) out[key] = await src.read();
      } catch (e) {
        addFault(faults, `${key}.profile`, e);
      }
    }
    if (!hasDsp()) faults.noDsp = true;
    for (const key of ['cf', 'sharc'] as const) {
      const n = restarts[key];
      if (n)
        faults.other.push(
          `${key}.profile: the firmware reloaded ${n} time${n === 1 ? '' : 's'} during the recording; counts cover only the time since the last reload.`,
        );
    }
    instructionNote = noteOf(faults);
    return out;
  }

  async function dspOf(
    s: ProfileSession,
    modules: ModuleRef[],
    sharc: Part | undefined,
    info: ProfileInfo,
  ): Promise<DspView | undefined> {
    if (!sharc) return undefined;
    const blocks = sharc.metrics.find((m) => m.name === 'sharc.blocks')?.value ?? 0;
    if (!(blocks > 0)) return undefined;
    let headroom: number | undefined;
    try {
      headroom = (await s.capacity()).DSP_COST?.limit || undefined;
    } catch {
      // the device does not report it: the bar is relative to the total
    }
    let report: Parameters<typeof dspView>[0] = { blocks, series: [] };
    const request = namesRequest;
    const inputs = nameInputs(info);
    if (deps.prof && inputs) {
      const doc = buildProfile({ run: { id: runId('emu', now()), target: device }, sharc });
      const prof = await deps.prof();
      if (request !== namesRequest || deps.names?.needsRefresh?.()) return dspView(report, modules, headroom);
      report = prof.profileReport({
        profile: doc,
        maps: inputs.maps,
        loads: inputs.loads,
        top: 40,
      }) as typeof report;
    }
    return dspView(report, modules, headroom);
  }

  async function capturedData(
    s: ProfileSession,
    info: ProfileInfo,
    all: Awaited<ReturnType<ProfileSession['profileAll']>>,
    parts: typeof kept,
  ): Promise<BuildInput> {
    const modules = await moduleRefs(s, info.port ?? '');
    const polls = await pollInfos(all.polls, info);
    const spans = spansPart(all, (x) => ownerName(x, modules, polls));
    const id = runId(deps.rpc ? 'emu' : 'dev', now());
    const target = (info.port ?? 'unknown').split('-')[0] ?? 'unknown';
    const run: BuildInput['run'] = { id, target };
    if (captureImage && !restarts.cf && !restarts.sharc && sameImage(captureImage.info, info)) {
      const current = await s.info().catch(() => undefined);
      if (current && sameImage(info, current)) run.firmware_sha256 = captureImage.sha256;
    }
    const capture: BuildInput = { run, ...parts, spans };
    if (traceGeneration !== undefined && !all.running) {
      try {
        capture.trace = await s.profileTrace(traceGeneration);
      } catch (error) {
        capture.traceError = asFault(error);
      }
    }
    return capture;
  }

  return {
    async read(detail = 'full') {
      const s = await deps.session();
      const info = await s.info();
      if (info.caps === null || ((info.caps >>> CAP.PEEK) & 1) === 0)
        return { noDebug: true, port: info.port ?? '', emulator: !!deps.rpc };
      const port = info.port ?? '';
      device = port.split('-')[0] ?? 'unknown';
      let all;
      try {
        all = await s.profileAll();
      } catch (e) {
        if (isDebugMissing(e)) return { noDebug: true, port, emulator: !!deps.rpc };
        throw e;
      }
      const observedAt = now().getTime();
      if (deps.names && (!namesImage || !sameImage(info, namesImage) || deps.names.needsRefresh?.()))
        await refreshNames(s, info);
      const modules = await moduleRefs(s, port);
      const snap: Snapshot = {
        running: all.running,
        hz: all.hz,
        elapsedTicks: all.elapsedTicks,
        spans: all.spans,
        polls: await pollInfos(all.polls, info),
      };
      const r: ProfileReading = {
        noDebug: false,
        port,
        emulator: !!deps.rpc,
        observedAt,
        imageId: info.image_id,
        snap,
        modules,
      };
      r.traceSupported = all.traceVersion === 1;
      if (!all.running) r.frozenTrace = frozenTrace;
      const trace = stoppedCapture?.trace;
      if (trace && !all.running) r.traceSummary = `${trace.events.length} trace events · ${trace.dropped} dropped`;
      if (stoppedCapture?.traceError && !all.running)
        r.traceSummary = `Trace unavailable · ${stoppedCapture.traceError}`;
      if (detail === 'spans') return r;
      if (deps.rpc) {
        if (counting() && all.running) {
          const parts = await instructionParts();
          try {
            r.dsp = await dspOf(s, modules, parts.sharc, info);
          } catch {
            // the DSP section stays empty
          }
          if (parts.cf) r.cf = await cfViewOf(parts.cf, info);
        }
        if (!hasDsp()) r.noDsp = true;
        if (instructionNote) r.note = instructionNote;
      } else {
        const h = await latestDeviceHealth(s).catch(() => undefined);
        if (h?.dsp != null && h.peak != null) r.deviceDsp = { avgPct: h.dsp, maxPct: h.peak };
      }
      return r;
    },

    async start() {
      const s = await deps.session();
      kept = {};
      stoppedCapture = undefined;
      frozenTrace = undefined;
      traceGeneration = undefined;
      captureImage = undefined;
      const info = await s.info();
      device = (info.port ?? '').split('-')[0] || device;
      await spansSource(s).start();
      await captureIdentity(s, info);
      restarts = {};
      if (!deps.rpc) return;
      active = {};
      const faults: ProfileFaults = { without: [], other: [] };
      const wanted: [keyof typeof active, string, Source<Part>][] = [['cf', 'cf.profile', cfSource()]];
      if (hasDsp()) wanted.push(['sharc', 'sharc.profile', sharcSource()]);
      else faults.noDsp = true;
      for (const [key, method, src] of wanted) {
        try {
          await src.start();
          active[key] = src;
        } catch (e) {
          addFault(faults, method, e);
        }
      }
      startFaults = faults;
      instructionNote = noteOf(faults);
    },

    async stop() {
      const s = await deps.session();
      let spansError: unknown;
      try {
        await spansSource(s).stop();
      } catch (e) {
        spansError = e; // the instruction sources still stop; the error comes after
      }
      if (deps.rpc && counting()) {
        kept = await instructionParts();
        const stopping = Object.entries(active) as [keyof typeof active, Source<Part>][];
        active = {};
        const faults: ProfileFaults = { without: [], other: [] };
        for (const [key, src] of stopping) {
          try {
            await src.stop();
          } catch (e) {
            addFault(faults, `${key}.profile stop`, e);
          }
        }
        const stopNote = noteOf(faults);
        if (stopNote) instructionNote = instructionNote ? `${instructionNote} ${stopNote}` : stopNote;
      }
      if (spansError !== undefined) throw spansError;
      if (captureImage || traceGeneration !== undefined) {
        try {
          const info = await s.info();
          const capture = await capturedData(s, info, await s.profileAll(), kept);
          if (capture.trace)
            frozenTrace = {
              trace: capture.trace,
              names: new Map(capture.spans?.spans.map((row) => [row.owner, row.name])),
            };
          if (capture.run.firmware_sha256 || capture.trace || capture.traceError) stoppedCapture = capture;
        } catch {
          // Export can still read the available counters, leaving the identity out if it cannot verify it.
        }
      }
    },

    async reset() {
      const s = await deps.session();
      const info = await s.info();
      const resetState = await state(s, 'reset');
      if (traceGeneration !== undefined)
        traceGeneration = resetState.running ? (await state(s, 'startTrace')).traceGeneration : undefined;
      kept = {};
      stoppedCapture = undefined;
      frozenTrace = undefined;
      await captureIdentity(s, info);
      restarts = {};
      try {
        for (const src of Object.values(active)) await src.reset();
      } catch (e) {
        captureImage = undefined;
        instructionNote = asFault(e);
      }
    },

    async exportFile() {
      let capture = stoppedCapture;
      if (!capture) {
        const s = await deps.session();
        const info = await s.info();
        const all = await s.profileAll();
        const parts = deps.rpc && counting() ? await instructionParts() : kept;
        capture = await capturedData(s, info, all, parts);
      }
      const doc = buildProfile(capture);
      return { text: `${JSON.stringify(doc)}\n`, name: `${capture.run.id}.dkprof.json` };
    },
  };
}

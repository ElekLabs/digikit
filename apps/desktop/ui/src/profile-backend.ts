// What the Profile pane reads and does (ProfilePanel.tsx): the runtime's span counters of the debug module over the target's session (dk-session
// profileState / profileAll, PROFILE_STATE and PROFILE_READ), and on the emulator also `cf.profile` and `sharc.profile` of the controller (the
// Host's `rpc`). One recorder per panel: no module state; the session, the rpc, the library and dkprof.wasm are arguments, so a test gives fakes.
// The preview replaces this module (preview/profile-backend.mock.ts). Host tests: tests/profile-backend.test.ts.
import { CAP, LOG_KIND } from '@digi/dk-session/gen_proto';
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
  type CfAnswer,
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
import type { LogNames } from '@digi/dk-session/log-names';

/** The part of the session the pane uses. */
export type ProfileSession = Pick<
  DkSession,
  'info' | 'modules' | 'capacity' | 'profileState' | 'profileAll' | 'logPage' | 'peek'
>;

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
      snap: Snapshot;
      modules: ModuleRef[];
      /** Emulator: the modules' DSP per block (undefined when `sharc.profile` is off or the daemon lacks it). */
      dsp?: DspView;
      /** Hardware: the newest HEALTH event, when the ring holds one. */
      deviceDsp?: DeviceDsp;
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
  names?: Pick<LogNames, 'inputs' | 'refresh'>;
  now?: () => Date;
}

/** The debug module is not there: no PEEK capability, or PROFILE_STATE is an unknown command of an older debug module. */
export const isDebugMissing = (e: unknown): boolean => e instanceof DkError && e.status === 'E_PROTO';

const KINDS: Record<LibraryModule['group'], OwnerKind> = {
  Machines: 'machine',
  Effects: 'effect',
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
  read(): Promise<ProfileReading>;
  /** Reset the counters and start the spans; on the emulator also `cf.profile` and `sharc.profile` count from now. */
  start(): Promise<void>;
  /** Stop the spans; on the emulator, keep the instruction profiles of the recording for Export. */
  stop(): Promise<void>;
  /** Zero the counters (both states); the recording window starts again. */
  reset(): Promise<void>;
  /** The `dk.profile/1` of what was recorded (spans, and on the emulator cf.profile and sharc.profile), as the file text and a file name. */
  exportFile(): Promise<{ text: string; name: string }>;
}

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
  /** What could not start for the running recording; read faults come on top. */
  let startFaults: ProfileFaults = { without: [], other: [] };

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
  const pollInfos = async (slots: (PollSlot | undefined)[]): Promise<(PollInfo | undefined)[]> => {
    const polls: (PollInfo | undefined)[] = slots.map((p) => (p ? { owner: p.owner, fn: p.fn } : undefined));
    const inputs = deps.names?.inputs();
    const used = polls.flatMap((p) => (p ? [p] : []));
    if (!deps.prof || !inputs || used.length === 0) return polls;
    try {
      const { results } = (await deps.prof()).addrLookup({
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
  const cfSource = (): Source<Part> => emuSource<CfAnswer, Part>('cf.profile', (a) => cfPart(undefined, a));
  const sharcSource = (): Source<Part> => emuSource<SharcAnswer, Part>('sharc.profile', sharcPart);

  const spansSource = (s: ProfileSession): Source<void> => ({
    start: async () => {
      await state(s, 'reset');
      await state(s, 'start');
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
    instructionNote = noteOf(faults);
    return out;
  }

  async function dspOf(s: ProfileSession, modules: ModuleRef[], sharc: Part | undefined): Promise<DspView | undefined> {
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
    const inputs = deps.names?.inputs();
    if (deps.prof && inputs) {
      const doc = buildProfile({ run: { id: runId('emu', now()), target: device }, sharc });
      report = (await deps.prof()).profileReport({
        profile: doc,
        maps: inputs.maps,
        loads: inputs.loads,
        top: 40,
      }) as typeof report;
    }
    return dspView(report, modules, headroom);
  }

  /** The newest HEALTH event of the ring (the whole-block DSP load the runtime logs), or undefined. */
  async function health(s: ProfileSession): Promise<DeviceDsp | undefined> {
    try {
      const [oldest, newest, first] = await s.logPage(0);
      let events = first;
      if (newest > oldest + first.length) events = (await s.logPage(Math.max(oldest, newest - 26)))[2];
      const h = [...events].reverse().find((e) => e.kind === LOG_KIND.HEALTH && ((e.args[0]! >>> 16) & 4) !== 0);
      return h ? { avgPct: h.args[1]! / 10, maxPct: h.args[2]! / 10 } : undefined;
    } catch {
      return undefined;
    }
  }

  return {
    async read() {
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
      const modules = await moduleRefs(s, port);
      const snap: Snapshot = {
        running: all.running,
        hz: all.hz,
        elapsedTicks: all.elapsedTicks,
        spans: all.spans,
        polls: await pollInfos(all.polls),
      };
      const r: ProfileReading = { noDebug: false, port, emulator: !!deps.rpc, snap, modules };
      if (deps.rpc) {
        if (counting() && all.running) {
          const parts = await instructionParts();
          try {
            r.dsp = await dspOf(s, modules, parts.sharc);
          } catch {
            // the DSP section stays empty
          }
        }
        if (!hasDsp()) r.noDsp = true;
        if (instructionNote) r.note = instructionNote;
      } else {
        const d = await health(s);
        if (d) r.deviceDsp = d;
      }
      return r;
    },

    async start() {
      const s = await deps.session();
      kept = {};
      device = ((await s.info()).port ?? '').split('-')[0] || device;
      await spansSource(s).start();
      if (!deps.rpc) return;
      active = {};
      try {
        await deps.names?.refresh(s); // the maps of the image that runs now (a reload changes them)
      } catch {
        // no names: the DSP view has the total only
      }
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
      await spansSource(s).stop();
      if (!deps.rpc || !counting()) return;
      kept = await instructionParts();
      const stopping = Object.values(active);
      active = {};
      for (const src of stopping) {
        try {
          await src.stop();
        } catch {
          // the daemon had nothing counting
        }
      }
    },

    async reset() {
      const s = await deps.session();
      await spansSource(s).reset();
      kept = {};
      try {
        for (const src of Object.values(active)) await src.reset();
      } catch (e) {
        instructionNote = asFault(e);
      }
    },

    async exportFile() {
      const s = await deps.session();
      const all = await s.profileAll();
      const info = await s.info();
      const modules = await moduleRefs(s, info.port ?? '');
      const parts = deps.rpc && counting() ? await instructionParts() : kept;
      const polls = await pollInfos(all.polls);
      const spans = spansPart(all, (x) => ownerName(x, modules, polls));
      const id = runId(deps.rpc ? 'emu' : 'dev', now());
      const doc = buildProfile({
        run: { id, target: (info.port ?? 'unknown').split('-')[0] ?? 'unknown' },
        ...parts,
        spans,
      });
      return { text: `${JSON.stringify(doc)}\n`, name: `${id}.dkprof.json` };
    },
  };
}

// Waits on the runtime's log (LOG) instead of fixed times, for any device: a cursor over the log pages (`LogCursor`) and the waits on it (`RuntimeEvents`).
//
// Cursor rules:
// - The cursor keeps the last event it returned (the anchor) and asks from the anchor's sequence number: the first event of the page must be the anchor
//   again (same seq, time, kind and arguments). Only sequence numbers order events.
// - A new boot restarts the log at sequence 1: the anchor is missing or differs, a BOOT_START follows it, or the log is shorter than the anchor. The cursor
//   then starts at the new boot's first event and `boots` counts it.
// - Two boots of the emulator can log the same events at the same numbers, so the anchor alone cannot tell them apart. The cursor also keeps the identity of its boot: the
//   BOOT_START at sequence 1 (its arguments hold the boot record's sequence number and the boot counter), read again with every `read` while the ring still holds it. Another
//   BOOT_START there is a new boot.
// - A gap (the oldest kept event is newer than the anchor in the same boot: the ring dropped events we had not read) is reported as `lost`, never thrown.
import { LOG_KIND } from './gen_proto.ts';
import type { DkInfoFull, DkLogEvent, DkSession, Quick } from './dkupload-core.ts';
import { nameHash } from './lifecycle-event.ts';
import { runtimeEvent, type RuntimeEvent } from './runtime-log.ts';
import { BOOT_WAIT_MS, type Clock, type WaitRecord } from './waits.ts';

export type LogKindName = keyof typeof LOG_KIND;

export interface LogPageSource {
  logPage(from: number, quick?: Quick): Promise<[oldest: number, newest: number, events: DkLogEvent[]]>;
}

const NEWEST = 0xffffffff;

const sameEvent = (a: DkLogEvent, b: DkLogEvent): boolean =>
  a.time === b.time && a.kind === b.kind && a.args.every((v, i) => v === b.args[i]);

export class LogCursor {
  private last: DkLogEvent | null = null;
  /** The BOOT_START (sequence 1) of the boot the cursor follows, once seen. */
  private boot: DkLogEvent | null = null;
  private count = 0;
  private readonly src: LogPageSource;
  private readonly probe: <T>(run: (quick: Quick) => Promise<T>) => Promise<T>;

  constructor(src: LogPageSource, opts: { probe?: <T>(run: (quick: Quick) => Promise<T>) => Promise<T> } = {}) {
    this.src = src;
    this.probe = opts.probe ?? (<T>(run: (quick: Quick) => Promise<T>) => run(false));
  }

  get boots(): number {
    return this.count;
  }

  private page(from: number) {
    return this.probe((q) => this.src.logPage(from, q));
  }

  /** The BOOT_START of the log's first sequence number while the ring still holds it. */
  private async bootStart(): Promise<DkLogEvent | null> {
    const [, , events] = await this.page(0);
    const head = events[0];
    return head !== undefined && head.seq === 1 && head.kind === LOG_KIND.BOOT_START ? head : null;
  }

  /** Start after the newest event: older events are never returned. */
  async fromNewest(): Promise<void> {
    this.last = null;
    this.boot = await this.bootStart();
    const [, newest] = await this.page(NEWEST);
    if (newest > 0) {
      const [, , events] = await this.page(newest);
      this.last = events.find((e) => e.seq === newest) ?? null;
    }
  }

  /** Start before sequence 1 of the current boot. */
  fromBoot(): void {
    this.last = null;
  }

  /** Everything new since the last read, in order. */
  async read(): Promise<{ events: DkLogEvent[]; newBoot: boolean; lost: number }> {
    let out: DkLogEvent[] = [];
    let newBoot = false;
    let lost = 0;
    let from = this.last ? this.last.seq : 0;
    let anchored = this.last !== null;
    // the identity of the boot: another BOOT_START at sequence 1 is a new boot, whatever the events at the anchor's number say
    const head = await this.bootStart();
    if (head !== null) {
      if (this.boot !== null && anchored && (head.time !== this.boot.time || !sameEvent(head, this.boot))) {
        this.count++;
        newBoot = true;
        this.last = null;
        from = 0;
        anchored = false;
      }
      this.boot = head;
    }
    for (;;) {
      const [oldest, newest, events] = await this.page(from);
      let batch = events;
      if (anchored) {
        const anchor = this.last!;
        const head = events[0];
        const same = head !== undefined && head.seq === anchor.seq && sameEvent(head, anchor);
        if (!same && newest >= anchor.seq && oldest > anchor.seq) {
          lost = oldest - (anchor.seq + 1);
          batch = events;
        } else {
          batch = same ? events.slice(1) : [];
          if (!same || batch.some((e) => e.kind === LOG_KIND.BOOT_START) || newest < anchor.seq) {
            this.count++;
            newBoot = true;
            this.last = null;
            this.boot = null;
            out = [];
            lost = 0;
            from = 0;
            anchored = false;
            continue;
          }
        }
        anchored = false;
      }
      out.push(...batch);
      if (batch.length > 0) this.last = batch[batch.length - 1]!;
      if (batch.length === 0 || this.last === null || this.last.seq >= newest) break;
      from = this.last.seq + 1;
    }
    return { events: out, newBoot, lost };
  }
}

/** The first argument (`cmd`) of an UPLOAD event that is no command id: the runtime lists them in `storage/upload_codes.rs`. */
export const UPLOAD_EVENT = {
  LOCK_BUSY: 0xb5,
  PENDING_DROPPED: 0xb6,
  PLUSDRIVE_REPLY: 0xb7,
  WRITE_FAILED: 0xb8,
  CLEAR_DROPPED_PACKAGE: 0xb9,
} as const;

/** `RuntimeEvents.until` ran out of time: the event was not seen. */
export class EventTimeout extends Error {}

/** The runtime refused or failed to load the module at `index` of the selection: `result` is its load code (E_IMPORT -7: `needs` names the core module the image lacks). */
export class ModuleLoadRefused extends Error {
  readonly index: number;
  readonly result: number;
  readonly needs: string | undefined;
  constructor(index: number, result: number, needs?: string) {
    super(
      `module ${index} of the selection did not load: result ${result}${needs ? `, the image lacks core/${needs}` : ''}`,
    );
    this.name = 'ModuleLoadRefused';
    this.index = index;
    this.result = result;
    this.needs = needs;
  }
}

export interface RuntimeEventsOptions {
  /** Real or clock milliseconds between polls, default 50. */
  pollMs?: number;
  onWait?: (r: WaitRecord) => void;
  /** Module names whose records `listen()` sets to `level`. */
  debugModules?: string[];
  level?: number;
  /** How many events a failure message shows, default 12. */
  tail?: number;
}

interface WaitOpts {
  withinMs?: number;
}

export class RuntimeEvents {
  private readonly dk: Pick<DkSession, 'logPage' | 'logConfig' | 'setLogConfig' | 'modules' | 'info'>;
  private readonly clock: Clock;
  private readonly cursor: LogCursor;
  private readonly pollMs: number;
  private readonly onWait: ((r: WaitRecord) => void) | undefined;
  private readonly debugModules: string[];
  private readonly level: number;
  private readonly tail: number;
  private queue: RuntimeEvent[] = [];
  private recent: RuntimeEvent[] = [];

  constructor(
    dk: Pick<DkSession, 'logPage' | 'logConfig' | 'setLogConfig' | 'modules' | 'info'>,
    clock: Clock,
    opts: RuntimeEventsOptions = {},
  ) {
    this.dk = dk;
    this.clock = clock;
    this.cursor = new LogCursor(dk);
    this.pollMs = opts.pollMs ?? 50;
    this.onWait = opts.onWait;
    this.debugModules = opts.debugModules ?? ['digikit/remote', 'digikit/debug'];
    this.level = opts.level ?? 3;
    this.tail = opts.tail ?? 12;
  }

  get boots(): number {
    return this.cursor.boots;
  }

  /** Cursor at the newest event (or before this boot's first) and the log levels of `debugModules` raised. */
  async listen({ fromBoot = false }: { fromBoot?: boolean } = {}): Promise<void> {
    this.queue = [];
    this.recent = [];
    // every debug-module command (LOG, LOGCFG) answers E_PROTO until the module has registered its range: INFO (a runtime command) says when its sink is set
    await this.untilDebugModule();
    if (fromBoot) this.cursor.fromBoot();
    else await this.cursor.fromNewest();
    await this.setLevels();
  }

  /** Waits on INFO flag bit 3 (the debug module's log sink; the module registers its command range before it sets the sink). */
  private async untilDebugModule(withinMs = 20_000): Promise<void> {
    const start = await this.clock.now();
    for (;;) {
      if (((await this.dk.info()) as Partial<DkInfoFull>).log_sink === true) return;
      if ((await this.clock.now()) - start >= withinMs)
        throw new EventTimeout(
          'the debug module (digikit/debug) did not set its log sink: INFO flag bit 3 stayed clear',
        );
      await this.clock.pause(this.pollMs);
    }
  }

  /** The records of `debugModules` that are loaded go to `level`; the other levels stay. */
  async setLevels(): Promise<void> {
    const want = new Set(this.debugModules);
    const config = await this.dk.logConfig();
    const modules = [...config.modules];
    for (const m of await this.dk.modules())
      if (want.has(m.name) && m.index < modules.length) modules[m.index] = this.level;
    await this.dk.setLogConfig(config.level, modules);
  }

  /** The events seen so far, one per line (the end of a failure message). */
  seen(): string {
    if (this.recent.length === 0) return '(no events seen)';
    return this.recent
      .slice(-this.tail)
      .map((e) => `#${e.seq} ${e.kind} ${e.args.map((a) => (a | 0).toString()).join(' ')}`)
      .join('\n');
  }

  async until(
    kind: LogKindName | LogKindName[] | '*',
    match: (e: RuntimeEvent) => boolean = () => true,
    { withinMs = 2000 }: WaitOpts = {},
  ): Promise<RuntimeEvent> {
    const start = await this.clock.now();
    const wallStart = Date.now();
    const done = async (ok: boolean) =>
      this.onWait?.({
        kind: 'event',
        what: Array.isArray(kind) ? kind.join('|') : kind,
        clockMs: (await this.clock.now()) - start,
        wallMs: Date.now() - wallStart,
        ok,
      });
    try {
      for (;;) {
        const hit = await this.scan(kind, match);
        if (hit) {
          await done(true);
          return hit;
        }
        if ((await this.clock.now()) - start >= withinMs)
          throw new EventTimeout(
            `events.until ${Array.isArray(kind) ? kind.join('|') : kind}: not seen within ${withinMs} ms. Last events:\n${this.seen()}`,
          );
        await this.clock.pause(this.pollMs);
      }
    } catch (e) {
      await done(false);
      throw e;
    }
  }

  /** `until`, but `null` when the event is not seen in time (a wait for the absence of an event). Other failures still throw. */
  async tryUntil(
    kind: LogKindName,
    match: (e: RuntimeEvent) => boolean = () => true,
    opts: WaitOpts = {},
  ): Promise<RuntimeEvent | null> {
    try {
      return await this.until(kind, match, opts);
    } catch (e) {
      if (e instanceof EventTimeout) return null;
      throw e;
    }
  }

  /**
   * After a boot or restart of a selection of `count` content modules: wait on the runtime's own events until every module of the selection has its result: MOD_LOAD
   * with 0, or with 1 and then MOD_DSP with 0 (the DSP part; `dsp: false` for a session without the DSP, where no DSP part completes: the CF part is final). A module
   * the runtime refused (a negative result) is `ModuleLoadRefused`; E_IMPORT has its MOD_NEEDS right after (the missing core module), which is waited for a moment and
   * is empty when it does not come. The cursor must be at the start of the boot (`listen({ fromBoot: true })`). Provider files (services) have no index here.
   */
  async untilModulesActive(
    count: number,
    { withinMs = 120_000, dsp: withDsp = true }: WaitOpts & { dsp?: boolean } = {},
  ): Promise<void> {
    if (count < 1)
      throw new Error(
        'untilModulesActive: the selection has no module, so the runtime logs no module event: the debug module file (digikit/debug) is missing from the selection or the library',
      );
    const E_IMPORT = -7;
    const result = new Map<number, number>();
    const dsp = new Set<number>();
    const needs = new Map<number, string>();
    const complete = () => {
      for (let i = 0; i < count; i++) if (!result.has(i) || dsp.has(i)) return false;
      return true;
    };
    await this.until(
      ['MOD_LOAD', 'MOD_DSP', 'MOD_NEEDS'],
      (e) => {
        if (e.fields.provider) return false;
        const index = Number(e.fields.index);
        if (index >= count) return false;
        if (e.kind === 'MOD_LOAD') {
          const r = Number(e.fields.result);
          if (r === 1 && withDsp) {
            result.set(index, r);
            dsp.add(index);
          } else result.set(index, r === 1 ? 0 : r);
        } else if (e.kind === 'MOD_DSP') {
          dsp.delete(index);
          result.set(index, Number(e.fields.result));
        } else needs.set(index, String(e.fields.needs));
        return complete();
      },
      { withinMs },
    );
    for (let i = 0; i < count; i++) {
      const r = result.get(i)!;
      if (r === 0) continue;
      if (r === E_IMPORT && !needs.has(i)) {
        // MOD_NEEDS follows its MOD_LOAD at once
        const e = await this.tryUntil('MOD_NEEDS', (x) => Number(x.fields.index) === i, { withinMs: 500 });
        if (e) needs.set(i, String(e.fields.needs));
      }
      throw new ModuleLoadRefused(i, r, needs.get(i) ?? (r === E_IMPORT ? '' : undefined));
    }
  }

  untilBoot(opts?: WaitOpts) {
    return this.until('BOOT_START', undefined, { withinMs: BOOT_WAIT_MS, ...opts });
  }
  untilUiFirst({ loaded, ...opts }: WaitOpts & { loaded?: number } = {}) {
    return this.until('UI_FIRST', (e) => loaded === undefined || Number(e.fields.loaded) >= loaded, {
      withinMs: BOOT_WAIT_MS,
      ...opts,
    });
  }
  untilModLoad(name: string, opts?: WaitOpts) {
    return this.until('MOD_LOAD', (e) => e.fields.name === name, { withinMs: 30_000, ...opts });
  }
  /** A step of the package lifecycle of the file `file` (its path or name): `phase` of `step`, such as `begin` of `place` (LIFECYCLE events; they come only while a debug module listens). */
  untilLifecycle(file: string, step: string, phase: 'begin' | 'end' | 'note', opts?: WaitOpts) {
    const hash = nameHash(file).toString(16).padStart(8, '0');
    return this.until(
      'LIFECYCLE',
      (e) => e.fields.hash === hash && e.fields.step === step && e.fields.phase === phase,
      { withinMs: 30_000, ...opts },
    );
  }
  untilModDsp(index: number, opts?: WaitOpts) {
    return this.until('MOD_DSP', (e) => e.fields.index === index, { withinMs: 30_000, ...opts });
  }
  /** The first UI-loop pass of this boot (FW_READY): the boot intro has ended and the stock MidiRpc service answers. */
  untilFwReady(opts?: WaitOpts) {
    return this.until('FW_READY', undefined, { withinMs: BOOT_WAIT_MS, ...opts });
  }
  untilSafe(opts?: WaitOpts) {
    return this.until('SAFE', undefined, { withinMs: BOOT_WAIT_MS, ...opts });
  }
  untilSettingsWrite(opts?: WaitOpts) {
    return this.until('SETTINGS_WRITE', undefined, { withinMs: 10_000, ...opts });
  }
  untilInputTaken(control: number, delta?: number, opts?: WaitOpts) {
    return this.until(
      'INPUT_TAKEN',
      (e) => e.args[0] === 2 && e.args[1] === control && (delta === undefined || (e.args[2]! | 0) === delta),
      { withinMs: 2000, ...opts },
    );
  }

  /** Read what is new and return the first matching event; later events stay queued for the next wait. */
  private async scan(
    kind: LogKindName | LogKindName[] | '*',
    match: (e: RuntimeEvent) => boolean,
  ): Promise<RuntimeEvent | null> {
    for (;;) {
      while (this.queue.length > 0) {
        const e = this.queue.shift()!;
        if (
          (kind === '*' || (Array.isArray(kind) ? kind.includes(e.kind as LogKindName) : e.kind === kind)) &&
          match(e)
        )
          return e;
      }
      const r = await this.cursor.read();
      if (r.newBoot) {
        this.recent = [];
        await this.setLevels().catch(() => undefined); // a module not registered yet: call listen() later
      }
      const events = r.events.map(runtimeEvent);
      for (const e of events) {
        this.recent.push(e);
        if (this.recent.length > 4 * this.tail) this.recent.shift();
      }
      if (r.lost > 0) throw new Error(`events: the log lost ${r.lost} events before they were read\n${this.seen()}`);
      if (events.length === 0) return null;
      this.queue = events;
    }
  }
}

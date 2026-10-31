// Following the runtime log of one origin (the emulator or a device): the pure part, with no timers and no Tauri. One `tick` asks the origin
// for the events it has not seen, and the follower says what it found as log entries. The rules and texts are those of `dklog.py follow`
// (`Follower.poll`); the Source menu, the timers and the sessions are in log-follow.ts. Host tests: apps/desktop/ui/tests/log-follow.test.ts.
import {
  faultDetail,
  faultSummary,
  eventDetail,
  eventLine,
  eventText,
  FAULT_DETAIL_KINDS,
  moduleOf,
  runtimeEvent,
  sameFault,
  type DkLogEvent,
  type Fault,
  type LogConfig,
} from '@digi/dk-session';
import { CAP, LOG_KIND, LOG_LEVELS } from '@digi/dk-session/gen_proto';
import type { LogInput, Origin } from './log-core.ts';
import { hasLogcfg } from './log-level-core.ts';

/** What the follower needs of a protocol session (a `DkSession` with `followSession`, or a fake). */
export interface FollowSession {
  info(quick?: boolean): Promise<{ caps: number | null }>;
  logPage(from: number, quick?: boolean): Promise<[number, number, DkLogEvent[]]>;
  logConfig(quick?: boolean): Promise<LogConfig>;
  /** LOGCFG set (the Level menu of the Log drawer). */
  setLogConfig(level: number, modules: number[], quick?: boolean): Promise<LogConfig>;
  /** LOGCFG op 2 (the Debug toggle of the Log drawer). */
  setDebug(on: boolean, quick?: boolean): Promise<LogConfig>;
  /** The loaded modules; `index` is the module record index. */
  modules(): Promise<{ index: number; name: string }[]>;
  /** The fault record the runtime keeps on the card (`dk/fault`), or null. */
  fault(): Promise<Fault | null>;
}

/** What the Source menu and the Follow button show of an origin. */
export type OriginStatus =
  /** Not followed (Follow is off, or nothing runs). */
  | 'off'
  | 'following'
  /** The origin answers INFO but has no LOG capability. */
  | 'no-runtime'
  /** The MIDI port is not there. */
  | 'absent'
  | 'frozen';

export interface FollowOut {
  push(inputs: LogInput[]): void;
  /** The names of the modules the origin has loaded changed. */
  modules(names: string[]): void;
  status(status: OriginStatus): void;
  /** The LOGCFG state the follower read (at the start, after a restart, after a DEBUG_SET event). */
  config?(config: LogConfig): void;
  /** Whether the origin's INFO has the LOGCFG capability (the Level menu works on it). */
  logcfg?(supported: boolean): void;
}

export interface FollowOptions {
  origin: Origin;
  session(): Promise<FollowSession>;
  /** Is the port (or the emulator) there? Asked when a request fails: a missing port is not a frozen device. */
  present?(): Promise<boolean>;
  now(): number;
  out: FollowOut;
}

/** No answer for this long: the device may be frozen. */
export const FROZEN_MS = 5000;
export const POLL_MS = 1000;
/** The events of the current boot that are kept for the detail of a fault. */
const KEEP = 1100;

const isCancelled = (e: unknown): boolean => (e as { kind?: string } | null)?.kind === 'cancelled';
const hasLog = (caps: number | null): boolean => caps !== null && ((caps >>> CAP.LOG) & 1) === 1;

export class LogFollower {
  /** Whether the follower waits for a firmware flash to end. */
  paused = false;
  private next = 0;
  private seenNewest = 0;
  private names = new Map<number, string>();
  private boot: DkLogEvent[] = [];
  private everStarted = false;
  private started = false;
  private lastAnswer: number;
  private frozen = false;
  private faultDue = false;
  private debugDue = false;
  private modulesDue = false;
  private lastDebug = '';
  private lastFault: Fault | null = null;
  private listed = '';

  constructor(private o: FollowOptions) {
    this.lastAnswer = o.now();
  }

  /** One round. Returns how long to wait before the next one. */
  async tick(): Promise<number> {
    try {
      const s = await this.o.session();
      if (!this.started && !(await this.start(s))) return FROZEN_MS;
      await this.poll(s);
      this.answered();
    } catch (e) {
      return this.failed(e);
    }
    return POLL_MS;
  }

  /** A firmware flash starts: the port belongs to the Firmware page. */
  pause(): void {
    if (this.paused) return;
    this.paused = true;
    this.say('paused during the firmware flash');
    this.o.out.status('off');
  }

  /** The flash is over: the device restarts, so its first answers may take a while. */
  resume(): void {
    this.paused = false;
    this.started = false;
    this.lastAnswer = this.o.now();
  }

  private say(text: string, level: 'info' | 'error' = 'info', detail?: string): void {
    this.o.out.push([
      { source: this.o.origin, level, text, ...(detail ? { detail } : {}), rt: { origin: this.o.origin, module: '' } },
    ]);
  }

  /** INFO decides whether there is a runtime log to follow. */
  private async start(s: FollowSession): Promise<boolean> {
    const info = await s.info(true);
    if (!hasLog(info.caps)) {
      this.o.out.status('no-runtime');
      this.lastAnswer = this.o.now();
      return false;
    }
    this.started = true;
    this.o.out.logcfg?.(hasLogcfg(info.caps));
    this.o.out.status('following');
    this.lastAnswer = this.o.now();
    if (!this.everStarted) {
      this.everStarted = true;
      this.faultDue = true;
    }
    this.debugDue = true;
    this.modulesDue = true;
    return true;
  }

  private answered(): void {
    if (this.frozen) {
      this.frozen = false;
      this.say('the device answers again');
      this.o.out.status('following');
    }
    this.lastAnswer = this.o.now();
  }

  private async failed(e: unknown): Promise<number> {
    if (isCancelled(e)) return POLL_MS;
    if (this.o.present && !(await this.o.present().catch(() => true))) {
      this.started = false;
      this.lastAnswer = this.o.now();
      this.o.out.status('absent');
      return FROZEN_MS;
    }
    if (!this.frozen && this.o.now() - this.lastAnswer >= FROZEN_MS) {
      this.frozen = true;
      this.say('no answer to LOG for 5 s: the device may be frozen (still asking)', 'info');
      this.o.out.status('frozen');
    }
    return this.frozen ? FROZEN_MS : POLL_MS;
  }

  /** Read what the origin has from `next` on; the rules of `Follower.poll`. */
  private async poll(s: FollowSession): Promise<void> {
    const rows: LogInput[] = [];
    try {
      for (;;) {
        const [oldest, newest, events] = await s.logPage(this.next, true);
        if (newest && newest < this.seenNewest) {
          this.o.out.push(rows.splice(0));
          this.say(`the ${this.o.origin} restarted: the log starts again (was at ${this.seenNewest}, now ${newest})`);
          this.next = 0;
          this.seenNewest = 0;
          this.boot = [];
          this.faultDue = this.debugDue = this.modulesDue = true;
          continue;
        }
        if (events.length === 0) break;
        if (events[0]!.seq > this.next && this.next)
          rows.push(this.note(`${events[0]!.seq - this.next} events lost (the ring held from ${oldest})`));
        for (const ev of events) this.take(ev, rows);
        this.next = events[events.length - 1]!.seq + 1;
        this.seenNewest = Math.max(this.seenNewest, newest);
        if (this.next > newest) break;
      }
    } finally {
      // rows read before an error are still shown, and not read again
      if (rows.length) this.o.out.push(rows.splice(0));
    }
    if (this.modulesDue) await this.readModules(s);
    if (this.debugDue) await this.readDebug(s);
    if (this.faultDue) await this.readFault(s);
  }

  private note(text: string): LogInput {
    return { source: this.o.origin, level: 'info', text, rt: { origin: this.o.origin, module: '' } };
  }

  private take(ev: DkLogEvent, rows: LogInput[]): void {
    if (ev.kind === LOG_KIND.MOD_LIST) {
      this.names.set(ev.args[0]! + 1, runtimeEvent(ev).fields.name as string);
      this.modulesDue = true;
    } else if (ev.kind === LOG_KIND.MOD_LOAD) this.modulesDue = true;
    else if (ev.kind === LOG_KIND.DEBUG_SET) this.debugDue = true;
    this.boot.push(ev);
    if (this.boot.length > KEEP) this.boot.splice(0, this.boot.length - KEEP);
    if (FAULT_DETAIL_KINDS.has(ev.kind)) return; // part of the fault entry's detail
    const d = runtimeEvent(ev);
    rows.push({
      source: this.o.origin,
      level: d.error ? 'error' : 'info',
      text: eventText(d, this.names),
      detail: eventDetail(d),
      rt: { origin: this.o.origin, module: moduleOf(d.source, this.names), event: d, line: eventLine(d, this.names) },
    });
  }

  /** Keep the call from stopping the round: a runtime without the command is a runtime without it. */
  private async aux<T>(call: () => Promise<T>): Promise<T | undefined> {
    try {
      return await call();
    } catch (e) {
      if (isCancelled(e)) throw e;
      return undefined;
    }
  }

  private async readModules(s: FollowSession): Promise<void> {
    this.modulesDue = false;
    const list = await this.aux(() => s.modules());
    if (list) for (const m of list) this.names.set(m.index + 1, m.name);
    const names = [...this.names].sort(([a], [b]) => a - b).map(([, name]) => name);
    if (names.join('\n') === this.listed) return;
    this.listed = names.join('\n');
    this.o.out.modules(names);
  }

  private async readDebug(s: FollowSession): Promise<void> {
    this.debugDue = false;
    const cfg = await this.aux(() => s.logConfig(true));
    if (!cfg) return;
    this.o.out.config?.(cfg);
    const text = cfg.debug
      ? `${this.o.origin}: debug logging on, level ${LOG_LEVELS[cfg.level] ?? cfg.level}`
      : `${this.o.origin}: debug logging off: the runtime keeps only errors (turn it on in the runtime's DEBUG setting)`;
    if (text === this.lastDebug) return;
    this.lastDebug = text;
    this.say(text);
  }

  private async readFault(s: FollowSession): Promise<void> {
    this.faultDue = false;
    const f = await this.aux(() => s.fault());
    if (!f || sameFault(f, this.lastFault)) return;
    this.lastFault = f;
    this.say(`fault: ${faultSummary(f)}`, 'error', faultDetail(f, this.boot, this.names));
  }
}

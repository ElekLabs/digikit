// A flash from the first message to the verdict, over operations the app gives (so the same run is tested with a fake device, run in the browser
// over Web MIDI, in the desktop app over midir, and run headless against the emulator). Rust paces it: `flash::Sender` (through patchkit.wasm)
// says when the next message goes out. The device sends no reply while it receives (finding 75), so the verdict comes from the restart:
// the port goes away and comes back, then INFO (runtime and image id) or, for a restore, SoftwareVersion.
import type { FlashSender, PreparedFlash, RpcMachine } from '../patch/patchkit-core';
import {
  CANCEL_TEXT,
  NO_ANSWER_TEXT,
  NO_RESTART_TEXT,
  NO_RETURN_TEXT,
  judge,
  sendFailedText,
  type Answer,
  type FlashState,
} from './firmware-core';

export interface FlowOps {
  /** The Rust sender for this many messages. */
  sender(total: number): Promise<FlashSender>;
  /** One message out; throws when the port refuses it. */
  send(message: Uint8Array): Promise<void> | void;
  /** A millisecond clock that does not go back. */
  now(): number;
  sleep(ms: number): Promise<void>;
  /** Let go of the port: the device is about to restart. */
  release(): Promise<void>;
  /** Is the device's port in the list now? */
  portPresent(): Promise<boolean>;
  /** Ask the device after the restart; throws while it does not answer. INFO that does not answer is `runtime: undefined`. */
  ask(): Promise<Answer>;
}

export interface FlowLimits {
  /** How long the port may take to go away after the last message. */
  leaveMs: number;
  /** How long it may take to come back. */
  returnMs: number;
  /** How long the device may take to answer after it is back. */
  askMs: number;
  pollMs: number;
  askEveryMs: number;
}

export const LIMITS: FlowLimits = { leaveMs: 240_000, returnMs: 240_000, askMs: 90_000, pollMs: 400, askEveryMs: 2000 };

const messageOf = (p: PreparedFlash, i: number): Uint8Array => p.image.subarray(i === 0 ? 0 : p.ends[i - 1], p.ends[i]);

const text = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** The UI is told at most this often while sending. */
const UI_MS = 100;

/**
 * Send `p`, wait for the restart, check the device. `set` receives every state; the last one is also returned. `signal` stops the run: while
 * sending, the sender is told to stop (the device keeps its old image, finding 75); while waiting, the wait ends.
 */
export async function runFlash(
  ops: FlowOps,
  p: PreparedFlash,
  signal: AbortSignal,
  set: (s: FlashState) => void,
  limits: FlowLimits = LIMITS,
): Promise<FlashState> {
  const done = (s: FlashState): FlashState => {
    set(s);
    return s;
  };
  const total = p.ends.length;
  const sender = await ops.sender(total);
  const startedMs = ops.now();
  try {
    let shown = Number.NEGATIVE_INFINITY;
    set({ phase: 'sending', sent: 0, total, startedMs, nowMs: startedMs });
    for (;;) {
      if (signal.aborted) {
        sender.fail();
        return done({ phase: 'stopped', reason: 'cancelled', sent: sender.sent(), total, message: CANCEL_TEXT });
      }
      const now = ops.now();
      const step = sender.next(now);
      if (step.kind === 'send') {
        try {
          await ops.send(messageOf(p, step.index));
        } catch (e) {
          sender.fail();
          return done({
            phase: 'stopped',
            reason: 'error',
            sent: step.index,
            total,
            message: sendFailedText(step.index, total, text(e)),
          });
        }
        if (now - shown >= UI_MS || step.index + 1 === total) {
          shown = now;
          set({ phase: 'sending', sent: step.index + 1, total, startedMs, nowMs: now });
        }
      } else if (step.kind === 'wait') {
        await ops.sleep(step.ms);
      } else if (step.kind === 'done') {
        break;
      } else {
        return done({ phase: 'stopped', reason: 'error', sent: sender.sent(), total, message: 'The sender stopped.' });
      }
    }
  } finally {
    sender.free();
  }
  return afterSend(ops, p, signal, set, limits, false);
}

/**
 * The device checks and writes by itself; its port goes away and comes back, then the device is asked. Shared by both routes (`rpc`: the
 * device accepted End with commit 1; menu: the last message was sent).
 */
async function afterSend(
  ops: Pick<FlowOps, 'now' | 'sleep' | 'release' | 'portPresent' | 'ask'>,
  p: PreparedFlash,
  signal: AbortSignal,
  set: (s: FlashState) => void,
  limits: FlowLimits,
  rpc: boolean,
): Promise<FlashState> {
  const done = (s: FlashState): FlashState => {
    set(s);
    return s;
  };
  // The device checks and writes by itself; its port goes away and comes back.
  const since = ops.now();
  const stage = (s: 'leaving' | 'returning' | 'asking') =>
    set({ phase: 'rebooting', stage: s, sinceMs: since, nowMs: ops.now(), ...(rpc ? { rpc: true as const } : {}) });
  stage('leaving');
  await ops.release();
  const wait = async (present: boolean, within: number, s: 'leaving' | 'returning'): Promise<boolean> => {
    const end = ops.now() + within;
    while (ops.now() < end) {
      if (signal.aborted) return false;
      if ((await ops.portPresent()) === present) return true;
      await ops.sleep(limits.pollMs);
      stage(s);
    }
    return false;
  };
  if (!(await wait(false, limits.leaveMs, 'leaving'))) {
    return signal.aborted
      ? done({ phase: 'failed', code: 'no-restart', message: 'You stopped waiting. The device carries on by itself.' })
      : done({ phase: 'failed', code: 'no-restart', message: NO_RESTART_TEXT });
  }
  stage('returning');
  if (!(await wait(true, limits.returnMs, 'returning'))) {
    return signal.aborted
      ? done({ phase: 'failed', code: 'no-return', message: 'You stopped waiting. The device carries on by itself.' })
      : done({ phase: 'failed', code: 'no-return', message: NO_RETURN_TEXT });
  }
  stage('asking');
  const end = ops.now() + limits.askMs;
  let stockSeen = 0;
  let last: Answer | undefined;
  const verdict = (a: Answer): FlashState => {
    const v = judge(p, a);
    return v.ok
      ? { phase: 'done', message: v.message, lines: v.lines }
      : { phase: 'failed', code: v.code, message: v.message };
  };
  while (ops.now() < end && !signal.aborted) {
    await ops.sleep(limits.askEveryMs);
    let a: Answer;
    try {
      a = await ops.ask();
    } catch {
      stage('asking');
      continue;
    }
    // A restore ends when the stock firmware answers and no runtime does, twice in a row: a patched image answers SoftwareVersion
    // a little before its runtime answers INFO.
    last = a;
    if (p.restore && !a.runtime && a.os === p.os && ++stockSeen < 2) continue;
    // a patched image answers SoftwareVersion before its runtime answers INFO: wait for the runtime
    if (!p.restore && !a.runtime) continue;
    return done(verdict(a));
  }
  if (last && !signal.aborted) return done(verdict(last));
  return done({
    phase: 'failed',
    code: 'no-answer',
    message: signal.aborted ? 'You stopped waiting.' : NO_ANSWER_TEXT,
  });
}

// ---- the RPC route ---------------------------------------------------------------------------------------------------------------

/** The operations of the RPC route: the state machine (Rust, `flash_rpc::RpcFlash`) and a port that sends one message and reads the next. */
export interface RpcOps extends Pick<FlowOps, 'send' | 'now' | 'sleep' | 'release' | 'portPresent' | 'ask'> {
  machine(image: Uint8Array, chunk: number): Promise<RpcMachine>;
  /** The next message the device sent within `timeoutMs`, or null; ends early (null) when `signal` aborts. */
  recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null>;
}

/**
 * Send the image through the RPC route, commit when the device has checked it, and wait for the restart. All decisions (what to send, how often to repeat a chunk, when a
 * run has failed) are the Rust machine's; this loop moves bytes and the clock. `signal` aborts: before End with commit 1 the device gets
 * End with commit 0 and keeps its flash. `chunk` 0 is the default size.
 */
export async function runRpcFlash(
  ops: RpcOps,
  p: PreparedFlash,
  signal: AbortSignal,
  set: (s: FlashState) => void,
  limits: FlowLimits = LIMITS,
  chunk = 0,
): Promise<FlashState> {
  const done = (s: FlashState): FlashState => {
    set(s);
    return s;
  };
  const total = p.image.length;
  const m = await ops.machine(p.image, chunk);
  const startedMs = ops.now();
  let rebooting = false;
  try {
    let shown = Number.NEGATIVE_INFINITY;
    let cancelled = false;
    let portError = '';
    const progress = (checking: boolean) => {
      const now = ops.now();
      if (checking || now - shown >= UI_MS || m.taken() === total) {
        shown = now;
        set({
          phase: 'sending',
          sent: m.taken(),
          total,
          startedMs,
          nowMs: now,
          unit: 'bytes',
          ...(checking ? { checking: true } : {}),
        });
      }
    };
    set({ phase: 'sending', sent: 0, total, startedMs, nowMs: startedMs, unit: 'bytes' });
    for (;;) {
      if (signal.aborted && !cancelled) {
        cancelled = true;
        m.cancel();
      }
      const a = m.next();
      if (a.kind === 'send') {
        // the long timeout is the last Write: the device checks the whole image before it answers
        if (a.timeoutMs >= 10_000 && !cancelled) progress(true);
        try {
          await ops.send(a.bytes);
        } catch (e) {
          // a port that refuses the message cannot be asked for anything else: end the exchange as a timeout
          portError = text(e);
          m.timeout();
          continue;
        }
        const end = ops.now() + a.timeoutMs;
        // wait for the reply to this request: the message is ours (the machine says so), or the time is up, or the user aborts
        const wait = async (sig: AbortSignal | undefined): Promise<'got' | 'timeout' | 'aborted'> => {
          for (let left = end - ops.now(); left > 0; left = end - ops.now()) {
            const r = await ops.recv(left, sig);
            if (r && m.reply(r)) return 'got';
            if (!r && sig?.aborted) return 'aborted';
          }
          return 'timeout';
        };
        let r = await wait(cancelled ? undefined : signal);
        if (r === 'aborted') {
          cancelled = true;
          // End with commit 0 follows. When it is too late (End with commit 1 is in flight) the reply is still awaited.
          if (m.cancel()) continue;
          r = await wait(undefined);
        }
        if (r === 'timeout') m.timeout();
        if (r === 'got') progress(false);
      } else if (a.kind === 'ask') {
        // the Flash click was the yes: the device validated the image, so the commit follows at once (a cancel that arrived until now still wins)
        set({ phase: 'sending', sent: m.taken(), total, startedMs, nowMs: ops.now(), unit: 'bytes', flashing: true });
        m.confirm(!signal.aborted);
      } else if (a.kind === 'reboot') {
        rebooting = true;
        break;
      } else if (a.kind === 'done') {
        break;
      } else {
        const reason = a.code === 'cancelled' ? 'cancelled' : 'error';
        return done({
          phase: 'stopped',
          reason,
          sent: a.taken,
          total,
          unit: 'bytes',
          message: portError && reason === 'error' ? `${a.message} (${portError})` : a.message,
        });
      }
    }
  } finally {
    m.free();
  }
  return rebooting
    ? afterSend(ops, p, signal, set, limits, true)
    : done({ phase: 'failed', code: 'send', message: 'The device did not start the flash.' });
}

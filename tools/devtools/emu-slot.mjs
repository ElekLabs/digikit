// Machine-wide emulator slots: a command prefix for device and conformance targets.
//
//   node tools/devtools/emu-slot.mjs -- <command...>
//
// Every Nx launcher run has its own CPU budget, so several lanes at once can start more emulators than
// the machine runs well. This wrapper takes one of N slots shared by all worktrees of the clone
// (`<git common dir>/emu-slots/slot-<i>`), runs the command with inherited stdio, releases the slot and
// exits with the command's status (a signal n becomes 128+n). It waits without a time limit.
// N is the ceiling of tools/nx/resources.mjs (cores and RAM). A task has cores by DSP mode (DIGI_EMU_DSP, set by the Nx
// target from the runtimeAcceptance record: zero 1 core, sharc 3, device-tests.mjs taskMode). A slot is taken only when
// the cores of the running tasks plus this one fit the core budget (the performance cores, DIGI_BUDGET) and the machine has
// that many idle cores beside the reserve. Waiters are served by priority: the longest expected run first (its last
// duration, kept clone-wide beside the slots, else coupled before zero-DSP), so long coupled tasks start first and do not end the gate alone.
// One waiter at a time holds the `admit` lock, and decides. The first emulator is always admitted. After an admission the
// lock is kept for RAMP_MS, so the next decision sees the load of the new emulator.
// DIGI_EMU_SLOTS=N is a fixed count with no core, idle or order check (for an A/B).
//
// Lock rules are those of tests/regression/runtime/run-claim.ts: the lock file appears with the pid in it
// (written aside, then linked into place). A dead pid is taken over by renaming the lock away first. An
// unreadable lock counts as held until it is 10 minutes old. The admit lock is stale after ADMIT_MAX_MS even with a
// live pid. Release removes the lock only if it holds our pid.
import { spawn, spawnSync } from 'node:child_process';
import { constants } from 'node:os';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { retryDecision } from '../nx/retry.mjs';
import { MODE_CORES, RAMP_MS, budget, idleCores, reserve, workCores } from '../nx/resources.mjs';

const UNREADABLE_LOCK_MS = 10 * 60_000;
const POLL_MS = 500;
const MAX_TAIL = 2_000_000;

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code !== 'ESRCH';
  }
};

const lockText = (lock) => {
  try {
    return fs.readFileSync(lock, 'utf8');
  } catch {
    return undefined;
  }
};

// The admit lock is held for one decision plus RAMP_MS. Older than this, it is stale even if its pid lives:
// a holder that never releases it would otherwise block every waiter of the machine.
export const ADMIT_MAX_MS = 30_000;

const ageMs = (lock) => {
  try {
    return Date.now() - fs.statSync(lock).mtimeMs;
  } catch {
    return 0;
  }
};

// Whether the lock names a live process, or is too new to judge. A missing lock is not held. With `maxAgeMs`,
// a lock older than that is not held, whatever its pid.
const held = (lock, maxAgeMs) => {
  const text = lockText(lock);
  if (text === undefined) return false;
  if (maxAgeMs !== undefined && ageMs(lock) > maxAgeMs) return false;
  const pid = Number.parseInt(text, 10);
  if (Number.isInteger(pid) && pid > 0) return alive(pid);
  try {
    return Date.now() - fs.statSync(lock).mtimeMs < UNREADABLE_LOCK_MS;
  } catch {
    return false;
  }
};

let staleCount = 0;

export const tryLock = (lock, maxAgeMs) => {
  const tmp = `${lock}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, String(process.pid));
  try {
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        fs.linkSync(tmp, lock);
        return true;
      } catch (error) {
        if (error.code !== 'EEXIST') throw error;
      }
      if (held(lock, maxAgeMs)) return false;
      const seen = lockText(lock);
      const away = `${lock}.stale-${process.pid}-${++staleCount}`;
      try {
        fs.renameSync(lock, away);
      } catch {
        continue; // another process removed it first: try the link again
      }
      if (lockText(away) !== seen) {
        // We moved a live lock that was taken meanwhile: give it back.
        try {
          fs.linkSync(away, lock);
        } catch {
          /* taken again; the owner is not harmed */
        }
        fs.rmSync(away, { force: true });
        return false;
      }
      fs.rmSync(away, { force: true });
    }
    return false;
  } finally {
    fs.rmSync(tmp, { force: true });
  }
};

export const slotCount = (env = process.env) => {
  const set = env.DIGI_EMU_SLOTS;
  if (set !== undefined && set !== '') {
    const n = Number(set);
    if (!Number.isInteger(n) || n < 1) throw new Error(`DIGI_EMU_SLOTS must be a positive integer, got ${set}`);
    return n;
  }
  return budget(workCores(), env.DIGI_BUDGET).tasks;
};

// A fixed DIGI_EMU_SLOTS has no idle check.
const liveCheck = (env = process.env) => env.DIGI_EMU_SLOTS === undefined || env.DIGI_EMU_SLOTS === '';

const slotDir = () => {
  if (process.env.DIGI_EMU_SLOT_DIR) return path.resolve(process.env.DIGI_EMU_SLOT_DIR); // tests use their own
  const done = spawnSync('git', ['rev-parse', '--git-common-dir'], { encoding: 'utf8' });
  if (done.status !== 0) throw new Error('emu-slot: not in a git checkout');
  return path.join(path.resolve(done.stdout.trim()), 'emu-slots');
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const slotsInUse = (dir, slots) => {
  let used = 0;
  for (let i = 0; i < slots; i++) if (held(path.join(dir, `slot-${i}`))) used++;
  return used;
};

// Cores and priority of the task (env: DIGI_EMU_DSP).
export const taskCores = (env = process.env) => {
  const mode = env.DIGI_EMU_DSP || 'zero';
  if (!(mode in MODE_CORES)) throw new Error(`DIGI_EMU_DSP must be zero or sharc, got ${mode}`);
  return MODE_CORES[mode];
};

// Core budget of the machine for emulators: the budget B of tools/nx/resources.mjs (performance cores, or DIGI_BUDGET).
export const coreCapacity = (env = process.env) => budget(workCores(), env.DIGI_BUDGET).budget;

// Expected run seconds of a task when no earlier run is recorded: a coupled run is the long one.
const GUESS_SECONDS = { zero: 120, sharc: 600 };
// A waiter that does not fit may be overtaken by smaller ones at most this many times, so a coupled head is not starved.
export const MAX_OVERTAKE = 8;

// Seconds of the last passed run of each target, one file per target in `<slot dir>/durations/`: clone-wide, like the slots.
const durationFile = (dir, target) => path.join(dir, 'durations', target.replace(/[^\w.-]/g, '_'));
export const lastRunSeconds = (dir, target) => {
  const seconds = Number(lockText(durationFile(dir, target)));
  return seconds > 0 ? seconds : undefined;
};
export const recordRunSeconds = (dir, target, seconds) => {
  try {
    const file = durationFile(dir, target);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(`${file}.${process.pid}.tmp`, String(seconds));
    fs.renameSync(`${file}.${process.pid}.tmp`, file);
  } catch {
    /* an order hint is advice and never fails a task */
  }
};

const sumCores = (dir, slots) => {
  let cores = 0;
  for (let i = 0; i < slots; i++) {
    const text = lockText(path.join(dir, `slot-${i}`));
    if (text === undefined || !held(path.join(dir, `slot-${i}`))) continue;
    cores += Number(lockText(path.join(dir, `cores-${Number.parseInt(text, 10)}`))) || MODE_CORES.zero;
  }
  return cores;
};

// The live waiter that goes first: the highest priority, then the oldest. Dead waiters are removed.
let tickets = 0;
const firstWaiter = (dir) => {
  let best;
  for (const name of fs.readdirSync(dir)) {
    if (!name.startsWith('wait-') || name.endsWith('.tmp')) continue;
    const pid = Number.parseInt(name.slice(5), 10);
    if (!alive(pid)) {
      fs.rmSync(path.join(dir, name), { force: true });
      continue;
    }
    let ticket;
    try {
      ticket = JSON.parse(lockText(path.join(dir, name)));
    } catch {
      continue;
    }
    if (!best || ticket.priority > best.priority || (ticket.priority === best.priority && ticket.at < best.at))
      best = { ...ticket, pid, name };
  }
  return best;
};

// Take a slot, waiting as long as needed. `task`: { cores, priority, capacity }, the cores of this task, its order
// among the waiters and the core budget; without `live` (a fixed slot count) none of them counts. Returns { lock, admit }: the slot lock and the admit lock, which
// the caller releases at exit (it also expires by itself after the ramp interval).
export const acquire = async (
  dir,
  slots,
  live = true,
  log = (line) => console.error(line),
  task = { cores: MODE_CORES.zero, priority: 0, capacity: Infinity },
) => {
  fs.mkdirSync(dir, { recursive: true });
  const ticketName = `wait-${process.pid}.${++tickets}`;
  const ticket = path.join(dir, ticketName);
  if (live)
    fs.writeFileSync(
      ticket,
      JSON.stringify({ priority: task.priority, at: Date.now(), cores: task.cores, overtaken: 0 }),
    );
  const startedAt = Date.now();
  const admit = path.join(dir, 'admit');
  let told = false;
  const wait = async (used) => {
    if (!told) {
      told = true;
      log(`waiting for an emulator slot (${used} of ${slots} in use)`);
    }
    await sleep(POLL_MS);
  };
  for (;;) {
    if (!tryLock(admit, ADMIT_MAX_MS)) {
      await wait(slotsInUse(dir, slots));
      continue;
    }
    const used = slotsInUse(dir, slots);
    let idle;
    if (used >= slots) {
      release(admit);
      await wait(used);
      continue;
    }
    // The head waiter (the longest expected run) goes first. One that does not fit by cores yet may be overtaken by a
    // waiter that does, MAX_OVERTAKE times in all; then nothing overtakes and the running tasks drain until the head fits.
    let overtaking;
    const head = live ? firstWaiter(dir) : undefined;
    if (head && head.name !== ticketName) {
      const blocked = sumCores(dir, slots) + head.cores > task.capacity && head.overtaken < MAX_OVERTAKE;
      if (!blocked) {
        release(admit);
        await wait(used);
        continue;
      }
      overtaking = head;
    }
    // With no emulator running, work never starves: admit at once.
    if (live && used > 0) {
      if (sumCores(dir, slots) + task.cores > task.capacity) {
        release(admit);
        await wait(used);
        continue;
      }
      idle = await (task.idleCores ?? idleCores)();
      if (idle < (task.reserve ?? reserve()) + task.cores) {
        release(admit);
        await wait(used);
        continue;
      }
    }
    for (let i = 0; i < slots; i++) {
      const lock = path.join(dir, `slot-${i}`);
      if (!tryLock(lock)) continue;
      const idleText = idle === undefined ? '' : `, idle ${idle.toFixed(1)} cores`;
      if (overtaking) {
        const file = path.join(dir, overtaking.name);
        try {
          const { priority, at, cores, overtaken } = overtaking;
          fs.writeFileSync(file, JSON.stringify({ priority, at, cores, overtaken: overtaken + 1 }));
        } catch {
          /* the head is gone */
        }
      }
      if (live) {
        fs.writeFileSync(path.join(dir, `cores-${process.pid}`), String(task.cores));
        fs.rmSync(ticket, { force: true });
      }
      const waited = (Date.now() - startedAt) / 1000;
      log(`emulator slot taken (${used + 1} of ${slots}${idleText}), emulator slot waited ${Math.round(waited)} s`);
      // Keep the admit lock for the ramp interval; the exit handler releases it earlier if the command ends first.
      if (live) setTimeout(() => release(admit), RAMP_MS).unref();
      else release(admit);
      return { lock, admit };
    }
    release(admit);
    await wait(used);
  }
};

export const release = (lock) => {
  if (lockText(lock) === String(process.pid)) fs.rmSync(lock, { force: true });
};

// The files of this process beside the slot: its core count and its place in the queue.
const releaseOwn = (dir) => {
  try {
    for (const name of fs.readdirSync(dir))
      if (name === `cores-${process.pid}` || name.startsWith(`wait-${process.pid}.`))
        fs.rmSync(path.join(dir, name), { force: true });
  } catch {
    /* no slot directory yet */
  }
};

// One whole line per event in one append: safe with concurrent writers (JSON lines; tools/nx/gate-history.mjs converts).
const traceEvent = (name, startMs, endMs) => {
  const dir = process.env.DIGI_TRACE_DIR;
  if (!dir) return;
  const tid = process.env.NX_TASK_TARGET_PROJECT
    ? `${process.env.NX_TASK_TARGET_PROJECT}:${process.env.NX_TASK_TARGET_TARGET}`
    : path.basename(process.argv.slice(process.argv.indexOf('--') + 1).join(' ')).slice(0, 80);
  const event = { name, ph: 'X', ts: startMs * 1000, dur: (endMs - startMs) * 1000, pid: 'emulator slots', tid };
  try {
    fs.mkdirSync(dir, { recursive: true });
    fs.appendFileSync(path.join(dir, 'slots-trace.json'), `${JSON.stringify(event)}\n`);
  } catch {
    /* a trace is advice and never fails a task */
  }
};

// One JSON line per retry event in the gate trace dir; tools/nx/gate-history.mjs reads it.
const recordRetry = (event) => {
  const dir = process.env.DIGI_TRACE_DIR;
  if (!dir) return;
  try {
    fs.mkdirSync(dir, { recursive: true });
    fs.appendFileSync(path.join(dir, 'retries.jsonl'), `${JSON.stringify(event)}\n`);
  } catch {
    /* a record is advice and never fails a task */
  }
};

const main = async () => {
  const argv = process.argv.slice(2);
  const split = argv.indexOf('--');
  const cmd = split < 0 ? [] : argv.slice(split + 1);
  if (!cmd.length) {
    console.error('usage: node tools/devtools/emu-slot.mjs -- <command...>');
    process.exit(2);
  }
  let child;
  let signalled;
  const forward = (signal) => {
    signalled = signal;
    if (child) child.kill(signal);
    else process.exit(128 + constants.signals[signal]); // still waiting: no slot to release
  };
  process.on('SIGTERM', () => forward('SIGTERM'));
  process.on('SIGINT', () => forward('SIGINT'));
  const enteredAt = Date.now();
  const target = process.env.NX_TASK_TARGET_PROJECT
    ? `${process.env.NX_TASK_TARGET_PROJECT}:${process.env.NX_TASK_TARGET_TARGET}`
    : cmd.join(' ').slice(0, 80);
  const cores = taskCores();
  const dir = slotDir();
  const task = {
    cores,
    capacity: coreCapacity(),
    priority: lastRunSeconds(dir, target) ?? GUESS_SECONDS[cores > MODE_CORES.zero ? 'sharc' : 'zero'],
  };
  process.on('exit', () => releaseOwn(dir));
  const { lock, admit } = await acquire(dir, slotCount(), liveCheck(), undefined, task);
  const takenAt = Date.now();
  traceEvent('wait', enteredAt, takenAt);
  process.on('exit', () => {
    release(lock);
    release(admit);
  });
  if (signalled) process.exit(128 + constants.signals[signalled]);
  // The command runs with piped output (forwarded as it comes); the tail of it decides whether a failure is a
  // load symptom that earns one rerun (tools/nx/retry.mjs).
  const attempt = (n) => {
    let tail = '';
    child = spawn(cmd[0], cmd.slice(1), { stdio: ['inherit', 'pipe', 'pipe'] });
    child.on('error', (error) => {
      console.error(`emu-slot: ${error.message}`);
      process.exit(127);
    });
    const keep = (stream, sink) =>
      stream.on('data', (chunk) => {
        sink.write(chunk);
        tail = (tail + chunk).slice(-MAX_TAIL);
      });
    keep(child.stdout, process.stdout);
    keep(child.stderr, process.stderr);
    child.on('close', (code, signal) => {
      const status = code ?? 128 + (constants.signals[signal] ?? 0);
      const decision = signalled ? { retry: false } : retryDecision({ status, attempt: n, output: tail });
      if (decision.retry) {
        console.error(`emu-slot: RETRY ${target}: load symptom "${decision.symptom}"; rerunning once`);
        recordRetry({ target, symptom: decision.symptom, attempt: n, retried: true });
        attempt(n + 1);
        return;
      }
      if (n > 1) {
        const passed = status === 0;
        console.error(`emu-slot: retry of ${target} ${passed ? 'passed' : `failed (exit ${status})`}`);
        recordRetry({ target, attempt: n, passed });
      }
      traceEvent('run', takenAt, Date.now());
      if (status === 0 && liveCheck()) recordRunSeconds(dir, target, (Date.now() - takenAt) / 1000);
      process.exit(status);
    });
  };
  attempt(1);
};

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) await main();

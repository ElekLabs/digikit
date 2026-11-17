import { mkdtempSync, rmSync, utimesSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { ADMIT_MAX_MS, acquire, release } from '../../devtools/emu-slot.mjs';

const dirs: string[] = [];
const slotDir = () => {
  const dir = mkdtempSync(join(tmpdir(), 'emu-slot-'));
  dirs.push(dir);
  return dir;
};
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

// An admit lock that names a live process (our parent) but was written long ago.
const staleAdmit = (dir: string, ageMs: number) => {
  const admit = join(dir, 'admit');
  writeFileSync(admit, String(process.ppid));
  const then = (Date.now() - ageMs) / 1000;
  utimesSync(admit, then, then);
  return admit;
};

it('an admit lock older than ADMIT_MAX_MS is taken over even when its pid is alive', async () => {
  const dir = slotDir();
  staleAdmit(dir, ADMIT_MAX_MS + 5_000);
  const quiet = () => {};
  const got = await Promise.race([
    acquire(dir, 2, false, quiet),
    new Promise<null>((resolve) => setTimeout(() => resolve(null), 3_000)),
  ]);
  expect(got).not.toBeNull();
  expect(existsSync(got!.lock)).toBe(true);
  release(got!.lock);
  release(got!.admit);
});

it('a fresh admit lock of a live process still makes a waiter wait', async () => {
  const dir = slotDir();
  const admit = staleAdmit(dir, 0);
  let resolved = false;
  const pending = acquire(dir, 2, false, () => {}).then((got) => {
    resolved = true;
    return got;
  });
  await new Promise((resolve) => setTimeout(resolve, 1_500));
  expect(resolved).toBe(false);
  // The holder goes away: the waiter proceeds.
  rmSync(admit, { force: true });
  const got = await pending;
  expect(existsSync(got.lock)).toBe(true);
  release(got.lock);
  release(got.admit);
});

it('keeps tasks inside the core budget and serves the longer expected run first', async () => {
  const dir = slotDir();
  const quiet = () => {};
  const task = (priority: number) => ({ cores: 3, priority, capacity: 3 });
  const first = await acquire(dir, 4, true, quiet, task(0));
  const order: string[] = [];
  const take = (name: string, priority: number) =>
    acquire(dir, 4, true, quiet, task(priority)).then((got) => {
      order.push(name);
      return got;
    });
  // both wait: 3 busy cores of 3 leave no room. The one that arrived first is the shorter run.
  const short = take('short', 10);
  await new Promise((resolve) => setTimeout(resolve, 300));
  const long = take('long', 500);
  await new Promise((resolve) => setTimeout(resolve, 1_500));
  expect(order).toEqual([]);
  release(first.lock);
  release(first.admit);
  const got = await long;
  expect(order).toEqual(['long']);
  release(got.lock);
  release(got.admit);
  const next = await short;
  release(next.lock);
  release(next.admit);
}, 30_000);

it('lets a smaller waiter overtake a head that does not fit, a bounded number of times', async () => {
  const dir = slotDir();
  const quiet = () => {};
  const idle = async () => 100; // the machine is idle; only the core budget limits
  const task = (cores: number, priority: number) => ({ cores, priority, capacity: 4, idleCores: idle, reserve: 0 });
  const running = await acquire(dir, 8, true, quiet, task(3, 0));
  // the head needs 3 cores and does not fit beside 3 running; a 1-core waiter fits and goes first
  const head = acquire(dir, 8, true, quiet, task(3, 500));
  await new Promise((resolve) => setTimeout(resolve, 300));
  const small = await acquire(dir, 8, true, quiet, task(1, 1));
  expect(existsSync(small.lock)).toBe(true);
  // the head keeps its priority: it is admitted once the cores are free
  release(running.lock);
  release(small.lock);
  release(small.admit);
  const got = await head;
  expect(existsSync(got.lock)).toBe(true);
  release(got.lock);
  release(got.admit);
}, 30_000);

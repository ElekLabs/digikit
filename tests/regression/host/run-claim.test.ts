import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { claimDir, FAILED_KEEP, pruneKeyDirs, pruneSnapshotRoot, removeEntry, tryLock } from '../runtime/run-claim.ts';
import { snapshotStore } from '../runtime/devices.ts';

let base: string;
beforeEach(() => {
  base = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-claim-'));
});
afterEach(() => fs.rmSync(base, { recursive: true, force: true }));

const old = (file: string, ms: number) => fs.utimesSync(file, new Date(Date.now() - ms), new Date(Date.now() - ms));

test('a lock is taken once, with the pid in it from the start', () => {
  const lock = path.join(base, 'a.lock');
  expect(tryLock(lock)).toBe(true);
  expect(fs.readFileSync(lock, 'utf8')).toBe(String(process.pid));
  expect(tryLock(lock)).toBe(false);
  expect(fs.readdirSync(base)).toEqual(['a.lock']);
});

test('the lock of a dead process is taken over', () => {
  const lock = path.join(base, 'a.lock');
  fs.writeFileSync(lock, '2147483646');
  expect(tryLock(lock)).toBe(true);
});

test('an empty or unreadable lock is held until it is 10 minutes old', () => {
  for (const text of ['', 'junk']) {
    const lock = path.join(base, `${text || 'empty'}.lock`);
    fs.writeFileSync(lock, text);
    expect(tryLock(lock)).toBe(false);
    old(lock, 11 * 60_000);
    expect(tryLock(lock)).toBe(true);
  }
});

test('a claim wipes per-run files but keeps the daemon caches, and a release frees the dir', () => {
  const first = claimDir(base, 'dev-a', true);
  fs.mkdirSync(path.join(first.dir, 'cards'));
  fs.writeFileSync(path.join(first.dir, 'cards', 'snap'), 'x');
  fs.mkdirSync(path.join(first.dir, 'boot-1'));
  fs.writeFileSync(path.join(first.dir, 'daemon.stderr'), 'x');
  const second = claimDir(base, 'dev-a', true);
  expect(second.dir).not.toBe(first.dir);
  first.release();
  first.release();
  const again = claimDir(base, 'dev-a', true);
  expect(again.dir).toBe(first.dir);
  expect(fs.readdirSync(again.dir).sort()).toEqual(['.dirty', 'cards']);
  expect(fs.existsSync(path.join(again.dir, 'cards', 'snap'))).toBe(true);
});

test('a failed run dir is renamed, its snapshots are not reused, and only the newest ones stay', () => {
  for (let i = 0; i < FAILED_KEEP + 2; i++) {
    const claim = claimDir(base, 'dev-b', true);
    fs.mkdirSync(path.join(claim.dir, 'cards'));
    fs.writeFileSync(path.join(claim.dir, 'cards', 'snap'), String(i));
    claim.release(true);
    expect(fs.existsSync(claim.dir)).toBe(false);
    // `.failed-` names are unique
    for (const name of fs.readdirSync(base).filter((n) => n.includes('.failed-')))
      old(path.join(base, name), (FAILED_KEEP + 2 - i) * 1000);
  }
  const next = claimDir(base, 'dev-b', true);
  expect(fs.existsSync(path.join(next.dir, 'cards'))).toBe(false);
  expect(fs.readdirSync(base).filter((n) => n.includes('.failed-'))).toHaveLength(FAILED_KEEP);
});

test('a dir that was not released cleanly is not reused; a clean release removes the marker', () => {
  const clean = claimDir(base, 'dev-a', true);
  fs.mkdirSync(path.join(clean.dir, 'cards'));
  expect(fs.existsSync(path.join(clean.dir, '.dirty'))).toBe(true);
  clean.release();
  expect(fs.existsSync(path.join(clean.dir, '.dirty'))).toBe(false);
  const reused = claimDir(base, 'dev-a', true);
  expect(reused.dir).toBe(clean.dir);
  expect(fs.existsSync(path.join(reused.dir, 'cards'))).toBe(true);
  // the process dies: the lock is stale and the marker stays
  fs.writeFileSync(`${reused.dir}.lock`, '2147483646');
  const next = claimDir(base, 'dev-a', true);
  expect(next.dir).toBe(reused.dir);
  expect(fs.existsSync(path.join(next.dir, 'cards'))).toBe(false);
  expect(fs.readdirSync(base).filter((n) => n.includes('.failed-'))).toHaveLength(1);
});

test('a lock taken over from a dead process is not taken from its new owner, and release keeps a lock that is not ours', () => {
  const lock = path.join(base, 'a.lock');
  fs.writeFileSync(lock, '2147483646');
  expect(tryLock(lock)).toBe(true);
  expect(tryLock(lock)).toBe(false);
  expect(fs.readdirSync(base)).toEqual(['a.lock']);
  const claim = claimDir(base, 'dev-b', true);
  fs.writeFileSync(`${claim.dir}.lock`, String(process.ppid)); // another live process owns it now
  claim.release();
  expect(fs.readFileSync(`${claim.dir}.lock`, 'utf8')).toBe(String(process.ppid));
});

test('old daemon-key dirs go, except the newest 2 of each device and those with a live lock', () => {
  const keys = ['a', 'b', 'c', 'd', 'e', 'f'].map((c) => c.repeat(12));
  // a, b, c serve dt2 (c the oldest); d, e, f serve dn2 (f the oldest); b also holds a live lock
  keys.forEach((key, i) => {
    fs.mkdirSync(path.join(base, key, `${i < 3 ? 'dt2' : 'dn2'}-0`), { recursive: true });
    old(path.join(base, key), (i + 1) * 60_000);
  });
  old(path.join(base, keys[2]!), 10 * 60_000);
  old(path.join(base, keys[5]!), 11 * 60_000);
  pruneKeyDirs(base);
  expect(fs.readdirSync(base).sort()).toEqual([keys[0]!, keys[1]!, keys[3]!, keys[4]!].sort());
  fs.writeFileSync(path.join(base, keys[1]!, 'dt2-0.lock'), String(process.pid));
  expect(() => pruneKeyDirs(path.join(base, 'missing'))).not.toThrow();
});

const HOUR = 3600_000;

/** An entry of 100 bytes in `<root>/<key>/<device>/<name>`, last used AGE_MS ago. */
function entry(root: string, name: string, ageMs: number, key = 'k1', device = 'dev-a'): string {
  const dir = path.join(root, key, device, name);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'snap'), 'x'.repeat(100));
  old(dir, ageMs);
  return dir;
}

test('the run dirs of one claim base share a private snapshot store when no shared one is given', () => {
  const first = claimDir(base, 'dev-a', true);
  const second = claimDir(base, 'dev-a', true);
  expect(second.snapshotDir).toBe(first.snapshotDir);
  expect(first.snapshotDir.startsWith(base)).toBe(true);
});

test('a claim uses the shared store it is given', () => {
  const shared = path.join(base, 'shared', 'key', 'dev-b');
  expect(claimDir(base, 'dev-b', true, shared).snapshotDir).toBe(shared);
  expect(fs.statSync(shared).isDirectory()).toBe(true);
});

test('pruning removes the oldest old entries over the cap and keeps entries younger than 2 hours', () => {
  const root = path.join(base, 'snap-cache');
  entry(root, 'old', 30 * HOUR);
  entry(root, 'older', 40 * HOUR, 'k2', 'dev-b');
  entry(root, 'mid', 5 * HOUR);
  entry(root, 'fresh', HOUR / 2);
  entry(root, 'fresher', 1000, 'k2', 'dev-b');
  pruneSnapshotRoot(root, 350);
  expect(fs.readdirSync(path.join(root, 'k1', 'dev-a')).sort()).toEqual(['fresh', 'mid']);
  expect(fs.readdirSync(path.join(root, 'k2', 'dev-b'))).toEqual(['fresher']);
  // a cap of 0 removes every old entry and still keeps the young ones
  pruneSnapshotRoot(root, 0);
  expect(fs.readdirSync(path.join(root, 'k1', 'dev-a'))).toEqual(['fresh']);
  expect(fs.readdirSync(path.join(root, 'k2', 'dev-b'))).toEqual(['fresher']);
});

test('pruning leaves entries alone under the cap, removes old staging dirs and ignores dot dirs in the count', () => {
  const root = path.join(base, 'snap-cache');
  const kept = entry(root, 'old', 30 * HOUR);
  const dir = path.dirname(kept);
  const stale = path.join(dir, '.tmp-stem-1-2');
  const young = path.join(dir, '.tmp-stem-3-4');
  for (const staging of [stale, young]) {
    fs.mkdirSync(staging);
    fs.writeFileSync(path.join(staging, 'digi-audio-m5.snap'), 'x'.repeat(1000));
  }
  old(stale, 2 * HOUR);
  pruneSnapshotRoot(root, 150);
  expect(fs.readdirSync(dir).sort()).toEqual(['.tmp-stem-3-4', 'old']);
});

test('removing an entry leaves no dot dir behind and tolerates a missing one', () => {
  const dir = entry(path.join(base, 'snap-cache'), 'gone', 5 * HOUR);
  removeEntry(dir);
  expect(fs.readdirSync(path.dirname(dir))).toEqual([]);
  expect(() => removeEntry(dir)).not.toThrow();
});

test('the snapshot store is the same for every worktree of one git dir, and absent for a custom binary', () => {
  vi.stubEnv('DIGI_SNAP_STORE', '');
  try {
    const common = path.join(base, 'repo', '.git');
    const key = 'a'.repeat(64);
    const binary = path.join(common, 'emud-cache', 'dn2', key, 'digi-emud');
    const expected = path.join(common, 'snap-cache', key, 'dn2');
    expect(snapshotStore(binary, 'dn2')).toBe(expected);
    // the path of the store depends on the binary's git dir only, not on the worktree that runs the test
    expect(snapshotStore(binary, 'dt2')).toBe(path.join(common, 'snap-cache', key, 'dt2'));
    expect(snapshotStore(path.join(base, 'custom', 'digi-emud'), 'dn2')).toBeUndefined();
    vi.stubEnv('DIGI_SNAP_STORE', path.join(base, 'override'));
    expect(snapshotStore(binary, 'dn2')).toBe(path.join(base, 'override', key, 'dn2'));
  } finally {
    vi.unstubAllEnvs();
  }
});

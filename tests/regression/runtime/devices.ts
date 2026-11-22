import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { describe, test } from 'vitest';
import { cardCacheRoot, repoRoot, stockImage, type Device } from '@digi/test-support';
import { claimDir, newestFirst, pruneKeyDirs, type RunDirClaim } from './run-claim.ts';

/** What a file without `devices` runs on: the devices with a ready snapshot. A device with a cold boot only (`st`) is named by the file. */
const ALL_DEVICES: readonly Device[] = ['dt2', 'dn2'];
const KNOWN_DEVICES: readonly Device[] = ['dt2', 'dn2', 'st'];

/** `list` (default: both devices) narrowed to `DIGI_DEVICE` (`dt2` or `dn2`) when it is set. Every device loop of a device test goes through here. */
export function selectDevices(list: readonly Device[] = ALL_DEVICES): Device[] {
  const only = process.env.DIGI_DEVICE;
  if (only === undefined || only === '') return [...list];
  if (!KNOWN_DEVICES.includes(only as Device))
    throw new Error(`DIGI_DEVICE must be one of ${KNOWN_DEVICES.join(', ')}, got '${only}'`);
  return list.filter((device) => device === only);
}

/** The devices of this run: both, or the one in `DIGI_DEVICE`. */
export const DEVICES: readonly Device[] = selectDevices();

/** Registers a skipped test when `DIGI_DEVICE` removed every device of a file, so the file still has a test. Does nothing when `selected` is not empty. */
export function skipEmptyHalf(selected: readonly Device[], name: string): void {
  if (selected.length === 0)
    test.skip(`${name}: no device of this file is selected by DIGI_DEVICE=${process.env.DIGI_DEVICE}`, () => {});
}

/** `describe` for a file pinned to one device: skipped, without running `define`, when `DIGI_DEVICE` selects another device. */
export function describeOn(device: Device, name: string, define: () => void, skip = false): void {
  if (selectDevices([device]).length === 0) {
    describe.skip(name, () => {
      test('not selected by DIGI_DEVICE', () => {});
    });
  } else {
    describe.skipIf(skip)(name, define);
  }
}

export const FIRMWARE = { dt2: '1.17', dn2: '1.12', st: '1.42' } as const;

const claimedRunDirs = new Map<Device, { dir: string; snapshotDir: string }>();

/** The daemon build key: the parent directory of a binary in the shared `<git dir>/emud-cache/<dev>/<key>/digi-emud` store. A custom binary has none. */
export function daemonKey(binary: string): string | undefined {
  const parts = path.resolve(binary).split(path.sep);
  const at = parts.lastIndexOf('emud-cache');
  return at >= 0 && parts.length === at + 4 ? parts[at + 2] : undefined;
}

/**
 * `<git dir>/snap-cache/<daemon key>/<device>`: the boot snapshots of one daemon build, shared by every worktree (the daemon's
 * `DIGI_SNAPSHOT_DIR`). It sits next to the `emud-cache` of BINARY. `DIGI_SNAP_STORE` replaces `<git dir>/snap-cache` (tests only).
 * A custom binary has no build key: undefined.
 */
export function snapshotStore(binary: string, device: Device): string | undefined {
  const key = daemonKey(binary);
  if (!key) return undefined;
  const override = process.env.DIGI_SNAP_STORE;
  if (override) return path.join(override, key, device);
  const parts = path.resolve(binary).split(path.sep);
  const at = parts.lastIndexOf('emud-cache');
  return path.join(parts.slice(0, at).join(path.sep) || path.sep, 'snap-cache', key, device);
}

/** Card dirs kept in the card cache (newest by mtime). */
const CARDS_KEEP = 32;
/** A card dir touched within this time is never evicted: another worker may have loaded it. */
const CARDS_MIN_AGE_MS = 120 * 60_000;
/** A work or temp dir of a card build older than this is a leftover of a killed run. */
const WORK_MAX_AGE_MS = 60 * 60_000;

/** Evict card dirs of the card cache beyond the newest CARDS_KEEP (cards are touched on reuse), and the leftovers of killed builds. */
export function evictCards(): void {
  const root = cardCacheRoot();
  if (!fs.existsSync(root)) return;
  for (const entry of newestFirst(root).slice(CARDS_KEEP))
    if (Date.now() - entry.mtime > CARDS_MIN_AGE_MS) fs.rmSync(entry.full, { recursive: true, force: true });
  for (const name of fs.readdirSync(root).filter((n) => n.startsWith('.work-'))) {
    const full = path.join(root, name);
    try {
      if (Date.now() - fs.statSync(full).mtimeMs > WORK_MAX_AGE_MS) fs.rmSync(full, { recursive: true, force: true });
    } catch {
      /* removed meanwhile */
    }
  }
}

function sessionBase(device: Device): string | undefined {
  const key = daemonKey(daemonBinary(device));
  return key ? path.join(repoRoot(), 'dist/session-cache', key.slice(0, 12)) : undefined;
}

/** One run dir per device and vitest worker process at a time, so no two daemons share it. Dirs are reused
 * across processes (a pid lock file marks a dir as claimed). With a daemon from the shared build store the dirs sit below the daemon's
 * build key (`dist/session-cache/<key12>/<device>-<n>`). The booted-snapshot store they use is the one of the daemon build in the git dir
 * (`snapshotStore`, see run-claim.ts): snapshots carry no version, so a new daemon build must not load those of an older one.
 * Used by `liveOptions` (not wiped). */
export function cachedRunDir(device: Device): { dir: string; snapshotDir: string } {
  const known = claimedRunDirs.get(device);
  if (known) return known;
  const { dir, snapshotDir } = claimDir(
    sessionBase(device) ?? path.join(repoRoot(), 'dist/session-cache'),
    device,
    false,
    snapshotStore(daemonBinary(device), device),
  );
  const claim = { dir, snapshotDir };
  claimedRunDirs.set(device, claim);
  return claim;
}

/**
 * The run dir of one daemon start of a runtime fixture that wants snapshot restores: a persistent dir below the daemon's build key,
 * wiped of everything but the daemon's caches. Release it when the session has closed (`failed`: the dir is renamed and not reused).
 * Without a build key (a custom `DIGI_EMUD_<DEV>`) it gives undefined: the session uses a fresh temporary dir.
 */
export function claimRunDir(device: Device): RunDirClaim | undefined {
  const base = sessionBase(device);
  if (!base) return undefined;
  const claim = claimDir(base, device, true, snapshotStore(daemonBinary(device), device));
  pruneKeyDirs(path.dirname(base));
  evictCards();
  return claim;
}

const coldDaemons = new Map<Device, string>();

/**
 * The daemon of a device that boots cold (no ready snapshot, so no entry in `dist/daemons.json`): the plain build of `tools.emulator.native_emu --daemon --build-only`
 * on its stock image (a cargo no-op when nothing changed). Its last output line is the binary.
 */
function coldDaemon(device: Device): string {
  const known = coldDaemons.get(device);
  if (known) return known;
  const out = execFileSync(
    'uv',
    ['run', 'python', '-m', 'tools.emulator.native_emu', '--daemon', '--build-only', stockImage(device)],
    {
      cwd: repoRoot(),
      encoding: 'utf8',
      timeout: 3_600_000,
    },
  );
  const binary = out.trim().split('\n').at(-1)!;
  if (!fs.existsSync(binary)) throw new Error(`the ${device} daemon build printed no binary: ${out.slice(-300)}`);
  coldDaemons.set(device, binary);
  return binary;
}

/** The device daemon: `DIGI_EMUD_<DEV>`, else the same key in `dist/daemons.json` (written by `digi-control:daemons`). */
export function daemonBinary(device: Device): string {
  const key = `DIGI_EMUD_${device.toUpperCase()}`;
  const fromEnv = process.env[key];
  if (fromEnv) return fromEnv;
  if (!ALL_DEVICES.includes(device)) return coldDaemon(device);
  const file = path.join(repoRoot(), 'dist/daemons.json');
  if (fs.existsSync(file)) {
    const value = (JSON.parse(fs.readFileSync(file, 'utf8')) as Record<string, string>)[key];
    if (value) return value;
    throw new Error(
      `${key} unset and dist/daemons.json has no ${key} (no ${device} firmware or ready inputs when the daemons were built): run mise run emu:ready -- ${device}, then pnpm nx:pilot run digi-control:daemons`,
    );
  }
  throw new Error(`${key} unset and dist/daemons.json missing: run pnpm nx:pilot run digi-control:daemons`);
}

/** Full-tier scenarios run only under `DIGIKIT_TIER=full` (`verify --full`), as the Python tiers do. */
export const FULL_TIER = process.env.DIGIKIT_TIER === 'full';

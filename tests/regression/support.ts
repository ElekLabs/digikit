import fs from 'node:fs';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { afterAll, beforeAll, beforeEach, expect } from 'vitest';
import { DkSession, EmuUsbTransport } from '@digi/dk-session';
import { repoRoot, Session, type Device, type SessionOptions } from '@digi/test-support';
import { setTimingContext, timePhase, timingContext } from './timing.ts';

export interface SessionHandle {
  /** The session of this test file; valid inside tests and hooks. */
  readonly session: Session;
}

/** One daemon per test file: started in beforeAll, closed in afterAll. Unless `restart: false`, each test
 * starts from `session.restart()`. */
export function withSession(opts: SessionOptions & { restart?: boolean }): SessionHandle {
  let session: Session | undefined;
  beforeAll(async () => {
    session = await Session.start(opts);
  });
  afterAll(async () => {
    await session?.close();
  });
  beforeEach(async () => {
    if (opts.restart !== false) await session!.restart();
  });
  return {
    get session() {
      if (!session) throw new Error('session not started');
      return session;
    },
  };
}

export interface PatchedSession {
  readonly session: Session;
  readonly dk: DkSession;
  close(): Promise<void>;
}

/** A fresh runtime daemon; protocol cancellation precedes daemon shutdown. No remote module is assumed installed. */
export async function startPatchedSession(
  opts: Omit<SessionOptions, 'runtime' | 'dsp'> & { patchSet?: string },
  scope = 'patched runtime',
): Promise<PatchedSession> {
  const context = { scope, device: opts.device, session: randomUUID() };
  const session = await timePhase(
    'session.startup',
    () =>
      Session.start({
        ...opts,
        dsp: 'sharc',
        runtime: { patchSet: opts.patchSet },
      }),
    context,
  );
  setTimingContext(session, context);
  try {
    const dk = new DkSession(new EmuUsbTransport((method, params) => session.client.call(method, params)));
    let closing: Promise<void> | undefined;
    return {
      session,
      dk,
      close() {
        return (closing ??= timePhase(
          'session.shutdown',
          async () => {
            dk.cancelPending();
            await session.close();
          },
          context,
        ));
      },
    };
  } catch (err) {
    await timePhase('session.shutdown', () => session.close(), context);
    throw err;
  }
}

/** Boot identity and health only: runtime patch sets do not install remote-control modules. */
export async function expectRuntimeBoot({ session, dk }: PatchedSession): Promise<void> {
  await timePhase(
    'boot.validation',
    async () => {
      const identity = session.loadResult.runtime;
      expect(identity?.patched).toBe(true);
      if (!identity?.patched) throw new Error('stock input did not produce a runtime patch identity');
      expect(identity.set).toBe(session.options.runtime?.patchSet ?? 'emulator');
      expect(identity.base_sha256).toMatch(/^[a-f0-9]{64}$/);
      expect(identity.modules.length).toBeGreaterThan(0);
      expect(identity.cache_file).toBeTruthy();
      expect(session.loadResult.start_used).toBeTruthy();
      await expect.poll(async () => (await session.status()).cf.ready, { timeout: 20_000 }).toBe(true);
      const status = await session.status();
      expect(status.mode).toBe('free');
      expect(status.cf.ready).toBe(true);
      expect(status.firmware?.runtime).toEqual(identity);
      expect(dk.rpcChunk).toBe(16384);
      await session.expectNoFault('runtime boot');
    },
    timingContext(session),
  );
}

/** No restart hook: every acceptance file owns a single fresh, free-running runtime. Vitest forbids `expect.poll` in
 * `beforeAll`, so each test calls `expectRuntimeBoot` itself. */
export function withPatchedSession(
  opts: Omit<SessionOptions, 'runtime' | 'dsp'> & { patchSet?: string },
  scope?: string,
): {
  readonly runtime: PatchedSession;
} {
  let runtime: PatchedSession | undefined;
  beforeAll(async () => {
    runtime = await startPatchedSession(opts, scope);
  });
  afterAll(async () => {
    await runtime?.close();
  });
  return {
    get runtime() {
      if (!runtime) throw new Error('runtime not started');
      return runtime;
    },
  };
}

export interface FreeRegion {
  name: string;
  lo: number;
  hi: number;
}

/** Free ColdFire regions of devices/ports/<device>-<firmware>.toml: every `[[region]]` block with
 * `cpu = "cf"` (e.g. `cf.flash_cave`, `cf.expansion`). Only the keys `name`, `cpu`, `start`, `end`
 * and `excludes` (a list of `[lo, hi]` pairs, cut out of the region) are read, by regex on the block
 * text; everything else in the file is ignored. `hi` is the file's `end`. */
export function freeRegions(device: Device, firmware: string): FreeRegion[] {
  const file = path.join(repoRoot(), 'devices', 'ports', `${device}-${firmware}.toml`);
  const text = fs.readFileSync(file, 'utf8');
  const out: FreeRegion[] = [];
  for (const block of text.split(/^\[\[/m).slice(1)) {
    if (!block.startsWith('region]]')) continue;
    const str = (key: string) => new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm').exec(block)?.[1];
    const num = (key: string) => {
      const m = new RegExp(`^${key}\\s*=\\s*(0x[0-9a-fA-F]+|\\d+)`, 'm').exec(block);
      return m ? Number(m[1]) : undefined;
    };
    const name = str('name');
    const start = num('start');
    const end = num('end');
    if (str('cpu') !== 'cf') continue;
    if (name === undefined || start === undefined || end === undefined) {
      throw new Error(`${file}: malformed region block`);
    }
    const excludes: [number, number][] = [];
    const exc = /^excludes\s*=\s*(\[.*\])\s*$/m.exec(block)?.[1] ?? '[]';
    for (const m of exc.matchAll(/\[\s*(0x[0-9a-fA-F]+|\d+)\s*,\s*(0x[0-9a-fA-F]+|\d+)\s*\]/g)) {
      excludes.push([Number(m[1]), Number(m[2])]);
    }
    excludes.sort((a, b) => a[0] - b[0]);
    let lo = start;
    for (const [xl, xh] of excludes) {
      if (xl > lo) out.push({ name, lo, hi: xl });
      lo = Math.max(lo, xh);
    }
    if (lo < end) out.push({ name, lo, hi: end });
  }
  if (out.length === 0) throw new Error(`${file}: no cf regions found`);
  return out;
}

/** Arm the guard on the free regions of the device and firmware; no PC is allowed (stock firmware). */
export async function armFreeRegionGuard(session: Session): Promise<void> {
  const { device, firmware } = session.options;
  const protect = freeRegions(device, firmware).map((r) => ({ lo: r.lo, hi: r.hi, label: r.name }));
  await session.client.guardSet({ protect, allow_pc: [] });
}

/** Fails when the guard saw a write into a free region; lists the first hits. */
export async function expectNoGuardHits(session: Session): Promise<void> {
  const report = await session.client.guardReport();
  if (report.total === 0) return;
  const hex = (n: number) => `0x${n.toString(16)}`;
  const lines = report.hits
    .slice(0, 8)
    .map((h) => `${h.label} pc=${hex(h.pc)} addr=${hex(h.addr)} size=${h.size} icount=${h.icount}`);
  throw new Error(`guard: ${report.total} write(s) into free regions:\n${lines.join('\n')}`);
}

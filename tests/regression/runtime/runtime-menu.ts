import { fmtVersion, type DkLogEvent, type DkSession } from '@digi/dk-session';
import type { Session } from '@digi/test-support';
import { expect } from 'vitest';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

type Tap = (name: string, settleMs?: number) => Promise<void>;

/** Known menu states, independent of the currently displayed parameter or encoder target. */
export async function openDigikitMenu(tap: Tap, checkpoint?: (name: string) => Promise<void>): Promise<void> {
  await tap('NO', 2000);
  await tap('SETTINGS', 3000);
  for (let i = 0; i < 8; i++) await tap('DOWN');
  await checkpoint?.('settings-ninth-row');
  await tap('YES', 3000);
  await checkpoint?.('digikit-main');
}

export async function openRuntimeAbout(tap: Tap): Promise<void> {
  // ABOUT is the first row of the DIGIKIT menu
  await tap('YES', 3000);
}

export async function readAboutRows(session: Session, plan: RuntimePlan): Promise<string[]> {
  const base = planSymbol(plan, 'DK_ABOUT');
  const count = await session.client.memPeek(planSymbol(plan, 'DK_ABOUT_N'));
  if (count === null || !Number.isInteger(count) || count < 1 || count > 24)
    throw new Error(`invalid ABOUT row count: ${count}`);
  const rows: string[] = [];
  for (let i = 0; i < count; i++) {
    const row = Buffer.alloc(24);
    for (let offset = 0; offset < 24; offset += 4) {
      const word = await session.client.memPeek(base + i * 24 + offset);
      if (word === null) throw new Error('ABOUT row is unmapped');
      row.writeUInt32BE(word >>> 0, offset);
    }
    rows.push(row.toString('latin1').split('\0')[0]!);
  }
  return rows;
}

/** Poll semantic menu content, bounded both in time and in the number of memory reads per poll. */
export async function expectRuntimeAbout(session: Session, dk: DkSession, plan: RuntimePlan): Promise<string[]> {
  const runtimeRow = `RUNTIME ${fmtVersion((await dk.info()).runtime)}`;
  let rows: string[] = [];
  await expect
    .poll(async () => (rows = await readAboutRows(session, plan)), {
      timeout: 10_000,
      interval: 500,
      message: 'ABOUT describes the live runtime',
    })
    .toContain(runtimeRow);
  return rows;
}

/** Stable identity/boot semantics only; other ABOUT rows may contain changing diagnostics. */
export function meaningfulAboutRows(rows: string[]): string[] {
  return rows.filter((row) => /^(DIGIKIT |RUNTIME |BOOT: )/.test(row));
}

/** The symbols `readSmallRing` and `readCrashSeen` need in the runtime plan. */
export const RING_SYMBOLS = ['DKRT_LOG', 'DKRT_LOG_COUNT', 'DKRT_CRASH'];

const RING_EVENTS = 32;

async function word(session: Session, address: number): Promise<number> {
  const value = await session.client.memPeek(address);
  if (value === null) throw new Error(`guest memory at ${address.toString(16)} is unmapped`);
  return value >>> 0;
}

/**
 * The runtime's own ring of boot and error events (`dlog.rs`, 32 events), read from guest memory. Safe mode loads no module, so the debug module's LOG is
 * not there after a stop or a crash; this ring is what the runtime itself keeps, and what a crash record takes its tail from.
 */
export async function readSmallRing(session: Session, plan: RuntimePlan): Promise<DkLogEvent[]> {
  const base = planSymbol(plan, 'DKRT_LOG');
  const count = await word(session, planSymbol(plan, 'DKRT_LOG_COUNT'));
  const events: DkLogEvent[] = [];
  for (let seq = Math.max(1, count - RING_EVENTS + 1); seq <= count; seq++) {
    const at = base + ((seq - 1) % RING_EVENTS) * 32;
    const w = await Promise.all([0, 1, 2, 3, 4, 5, 6, 7].map((k) => word(session, at + 4 * k)));
    if (w[0] !== seq) continue;
    events.push({ seq, time: w[1]!, kind: w[2]! >>> 16, flags: w[2]! & 0xffff, args: w.slice(3, 8) });
  }
  return events;
}

/** What the boot found of a crash record (`DKRT_CRASH`, crash.rs `CrashSeen`): the words that tests look at. */
export async function readCrashSeen(
  session: Session,
  plan: RuntimePlan,
): Promise<{ found: number; panic: number; tail: number }> {
  const base = planSymbol(plan, 'DKRT_CRASH');
  return {
    found: await word(session, base),
    panic: await word(session, base + 40),
    tail: await word(session, base + 44),
  };
}

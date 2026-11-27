import { runUntilFramesWithProgress, type Session } from '@digi/test-support';
import { expect } from 'vitest';
import { bounded, rpc } from './guest-access.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

// core/fx's published registry row: runtime/platform/fx/module.toml.
export const ROW_WORDS = 24;
export const FREE_PC = 0xffffffff;

export async function dspBytes(session: Session, address: number, length: number): Promise<Buffer> {
  session.requireDsp('dspBytes');
  const result = await rpc<{ hex: string; unknown: number }>(session, 'dsp.peek', { addr: address, len: length });
  expect(result.unknown, 'DSP bytes are mapped').toBe(0);
  const bytes = Buffer.from(result.hex, 'hex');
  expect(bytes.length).toBe(length);
  return bytes;
}

export async function dspWords(session: Session, address: number, count: number): Promise<number[]> {
  const bytes = await dspBytes(session, address, count * 4);
  return Array.from({ length: count }, (_, i) => bytes.readUInt32LE(i * 4));
}

export async function rows(session: Session, plan: RuntimePlan, slots: number): Promise<number[][]> {
  const words = await dspWords(session, planSymbol(plan, 'fx.rows'), ROW_WORDS * slots);
  return Array.from({ length: slots }, (_, i) => words.slice(i * ROW_WORDS, (i + 1) * ROW_WORDS));
}

export async function frames(session: Session): Promise<number> {
  session.requireDsp('frames (the DSP frame count)');
  const frame = (await bounded(() => session.status())).dsp_frames;
  if (typeof frame !== 'number') throw new Error('status lacks DSP frame count');
  return frame;
}

/** Run to absolute DSP frame `target`; fails only when a half emulated second makes no progress for PROGRESS_HANG_S. */
export async function runToFrame(session: Session, target: number): Promise<void> {
  await runUntilFramesWithProgress(session.client, () => frames(session), target);
}

/** Run `n` more DSP frames with the same progress guard. */
export async function runFrames(session: Session, n: number): Promise<void> {
  await runToFrame(session, (await frames(session)) + n);
}

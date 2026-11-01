import { button, encoder, type Session } from '@digi/test-support';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { advance, bounded, frames, pickSine, rpc, word } from './fx-live.ts';
import { routeProtocol, tap, type RouteProfile } from './fx-route-live.ts';

export async function halfword(session: Session, address: number): Promise<number> {
  const value = await word(session, address & ~3);
  const offset = address % 4;
  if (offset <= 2) return (value >>> (8 * (2 - offset))) & 0xffff;
  return ((value & 0xff) << 8) | ((await word(session, (address & ~3) + 4)) >>> 24);
}

export async function soundImage(session: Session, profile: RouteProfile, track: number): Promise<number[]> {
  const base = (await word(session, profile.fx.kit_ptr)) + profile.fx.sound_off + profile.fx.sound_stride * track;
  const result: number[] = [];
  for (let index = 0; index < Math.floor(profile.fx.sound_stride / 2); index++)
    result.push(await halfword(session, base + 2 * index));
  return result;
}

export async function amountShown(session: Session, profile: RouteProfile, track: number, cell = 0): Promise<number> {
  const base = (await word(session, profile.fx.kit_ptr)) + profile.fx.sound_off + profile.fx.sound_stride * track;
  return (await halfword(session, base + 0x14 + 2 * profile.fx.send_indices[cell]!)) >>> 8;
}

/** Legacy fast-event bursts, then single slow detents; preserve acceleration and settling. */
export async function setAmount(
  session: Session,
  profile: RouteProfile,
  enc: string,
  track: number,
  want: number,
  cell = 0,
) {
  for (let attempt = 0; attempt < 200; attempt++) {
    const got = await amountShown(session, profile, track, cell);
    if (got === want) {
      await advance(session, 40);
      return;
    }
    const error = want - got;
    const fast = Math.abs(error) > 12;
    for (let event = 0; event < (fast ? 2 : 1); event++) {
      await bounded(() => session.client.inputTurn(encoder(enc), (error > 0 ? 1 : -1) * (fast ? 8 : 1)));
      await advance(session, fast ? 2 : 40);
    }
    await advance(session, 100);
    await advance(session, 20);
  }
  throw new Error(`turning ${enc}: shows ${await amountShown(session, profile, track, cell)}, wanted ${want}`);
}

/** DT2 retains the legacy selected-type readback and adjacent-list-position retries. */
export async function selectSine(runtime: PatchedSession, profile: RouteProfile): Promise<void> {
  const hint = runtime.session.options.device === 'dt2' ? 7 : 5;
  const sine = (await routeProtocol(runtime, (dk) => dk.modules())).find((module) => module.name === 'test/sine');
  expect(sine, 'sine is registered').toBeDefined();
  const want = sine!.types[0];
  expect(want).toBeDefined();
  for (const downs of runtime.session.options.device === 'dt2'
    ? [hint, hint + 1, hint - 1, hint + 2, hint + 3]
    : [hint]) {
    await pickSine(runtime.session, downs);
    if (runtime.session.options.device !== 'dt2') return;
    if ((((await word(runtime.session, profile.track_type_word)) >>> 8) & 0xff) === want) return;
  }
  expect(((await word(runtime.session, profile.track_type_word)) >>> 8) & 0xff, 'selected DT2 sine type').toBe(want);
}

/** Already-selected input: legacy note boundary offset and 1500-frame hold, without reopening the machine list. */
export async function trackNote(runtime: PatchedSession, file: string, triggerFrame: number): Promise<void> {
  const { session } = runtime;
  const current = await frames(session);
  expect(current, 'legacy fixed trigger has its setup margin').toBeLessThan(triggerFrame - 20);
  await rpc(session, 'pcm.tap', { action: 'start' });
  try {
    await bounded(() => session.client.runUntil({ frames: triggerFrame, timeout_wall_s: 30 }));
    await bounded(() => session.client.step(44_000, { timeout_wall_s: 30 }));
    await rpc(session, 'pcm.tap', { action: 'capture', path: file });
    await bounded(() => session.client.inputButton(button('TRIG 1'), true));
    await bounded(() => session.client.runUntil({ frames: triggerFrame + 1500, timeout_wall_s: 30 }));
  } finally {
    try {
      await rpc(session, 'pcm.tap', { action: 'capture_stop' });
    } finally {
      await bounded(() => session.client.inputButton(button('TRIG 1'), false));
    }
  }
  await advance(session, 50);
  await bounded(() => session.expectNoFault('track send note'));
}

export async function selectTrack(session: Session, n: number) {
  await tap(session, `TRIG ${n}`, 400, 'TRK');
}

export async function kitMenu(session: Session, entry: number) {
  await tap(session, 'PRESET', 500);
  await tap(session, 'RIGHT', 200);
  for (let i = 0; i < 2; i++) await tap(session, 'UP', 100);
  for (let i = 0; i < entry; i++) await tap(session, 'DOWN', 100);
}

export async function kitSlotOne(session: Session) {
  for (let i = 0; i < 4; i++) await tap(session, 'UP', 80);
}

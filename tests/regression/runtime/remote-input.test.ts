import { setTimeout as delay } from 'node:timers/promises';
import { SERVICE_CMD } from '@digi/dk-session/gen_proto';
import { button, type Session } from '@digi/test-support';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { timePhase } from '../timing.ts';
import { runtimeTests, settleUi, tap as holdTap } from './live.ts';
import { captureRemoteOled, installRemote, prepareRemote, remoteTap, withRemoteRelease } from './remote.ts';
import { openDigikitMenu, openRuntimeAbout, expectRuntimeAbout } from './runtime-menu.ts';

/** KEY navigation opens semantic ABOUT content; TURN reports accepted signed deltas. */
async function expectRemoteKeys(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  // Preserve live_boot's device-specific pre-install settling, then the remote scenario's own settle.
  await settleUi(session);
  await withRemoteRelease(dk, async () => {
    const plan = await prepareRemote(runtime);
    await timePhase('settle.post-install', () => delay(10_000));
    expect((await dk.remoteState()).enabled).toBe(true);
    let shots = 0;
    const shot = (name: string) => captureRemoteOled(session, `${String(++shots).padStart(2, '0')}-${name}`);
    const tap = async (name: string, settleMs = 1200) => {
      await remoteTap(dk, name, settleMs);
      await shot(name.toLowerCase());
    };
    await shot('start');
    await openDigikitMenu(tap);
    await openRuntimeAbout(tap);
    await expectRuntimeAbout(session, dk, plan);
    for (let i = 0; i < 4; i++) await tap('NO', 2000);
    try {
      expect(await dk.remoteTurnOnce(1, 20)).toBe(20);
      expect(await dk.remoteTurnOnce(1, -20)).toBe(-20);
    } finally {
      await shot('turn-signed');
    }
  });
  await session.expectNoFault('remote keys');
}

/** Physical panel input remains available while the remote protocol is disabled. */
async function panelTap(session: Session, name: string, settleMs = 1200): Promise<void> {
  const code = button(name);
  await holdTap((down) => session.client.inputButton(code, down), 400, settleMs);
}

async function toggleRemote(session: Session): Promise<void> {
  await panelTap(session, 'NO', 2000);
  await panelTap(session, 'SETTINGS', 3000);
  for (let i = 0; i < 8; i++) await panelTap(session, 'DOWN');
  await panelTap(session, 'YES', 3000);
  for (let i = 0; i < 6; i++) await panelTap(session, 'DOWN');
  await panelTap(session, 'YES', 2000);
  await panelTap(session, 'NO', 2000);
}

/** One-for-one port of the full-tier Python remote-control negative scenario. */
async function expectRemoteControl(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  await installRemote(runtime);
  await timePhase('settle.post-install', () => delay(10_000));
  const cmd = (command: number, body: number[] = []) => dk.request(command, Uint8Array.from(body));
  const state = async () => {
    const reply = await cmd(SERVICE_CMD.REMOTE_STATE);
    return [reply.status, ...reply.body];
  };
  expect(await state()).toEqual([0, 0, 1, 30]);
  expect([
    (await cmd(SERVICE_CMD.REMOTE_KEY, [2, 1])).status,
    (await cmd(SERVICE_CMD.REMOTE_TURN, [1, 1])).status,
    (await cmd(SERVICE_CMD.REMOTE_RELEASE)).status,
  ]).toEqual([-65, -65, -65]);

  await toggleRemote(session);
  expect(await state()).toEqual([0, 1, 1, 30]);
  await withRemoteRelease(dk, async () => {
    expect([
      (await cmd(SERVICE_CMD.REMOTE_KEY, [2, 1])).status,
      (await cmd(SERVICE_CMD.REMOTE_KEY, [2, 0])).status,
      (await cmd(SERVICE_CMD.REMOTE_RELEASE)).status,
    ]).toEqual([0, 0, 0]);
    expect([
      (await cmd(SERVICE_CMD.REMOTE_KEY, [1])).status,
      (await cmd(SERVICE_CMD.REMOTE_KEY, [2, 2])).status,
      (await cmd(SERVICE_CMD.REMOTE_TURN, [0, 1])).status,
    ]).toEqual([-50, -50, -50]);
    expect((await cmd(SERVICE_CMD.REMOTE_KEY, [127, 1])).status).toBe(-1);
    const first = await cmd(SERVICE_CMD.REMOTE_TURN, [1, 100]);
    const immediate = await cmd(SERVICE_CMD.REMOTE_TURN, [1, 100]);
    expect([first.status, [...first.body]]).toEqual([0, [30]]);
    expect(immediate.status).toBe(0);
    expect(immediate.body.length).toBe(1);
    // Adjacent host requests may span a firmware refill; exact timing is covered by the C pacing unit.
    expect([0, 30]).toContain(immediate.body[0]);
    await timePhase('settle.remote-refill', () => delay(3000));
    const back = await cmd(SERVICE_CMD.REMOTE_TURN, [1, 156]);
    expect(back.status).toBe(0);
    expect(back.body.length).toBe(1);
    expect(256 - back.body[0]!).toBeGreaterThanOrEqual(1);
    expect(256 - back.body[0]!).toBeLessThanOrEqual(30);
  });
  // Release while enabled; disabled RELEASE must not be treated as successful cleanup.
  await toggleRemote(session);
  expect(await state()).toEqual([0, 0, 1, 30]);
  expect((await cmd(SERVICE_CMD.REMOTE_KEY, [2, 1])).status).toBe(-65);
  await session.expectNoFault('remote control');
}

runtimeTests('remote keys open ABOUT and TURN accepts signed deltas', expectRemoteKeys);
runtimeTests('remote switch guards commands and panel toggle enables paced input', expectRemoteControl, {
  patchSet: 'device',
});

import { createFakeTransferApp, createFakeSessions } from '@digi/workbench-core/transfer/fake-app.ts';
// The pure logic of the desktop Remote page (apps/desktop/ui/src/remote-core.ts).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, type DkSession } from '@digi/dk-session';
import {
  INFO_EVERY_MS,
  deviceOfPort,
  infoDue,
  isRemoteMissing,
  keyErrorLine,
  leaveCancels,
  remoteListed,
  remoteNotice,
  remotePaused,
  remoteRuntime,
  remoteStateWanted,
  stateProbeKey,
} from '../src/remote-core.ts';

test('the notice: nothing when the module is on, where to check it, how to load the module', () => {
  assert.equal(remoteNotice({ enabled: true, format: 1, turnLimit: 30 }), '');
  assert.match(remoteNotice({ enabled: false, format: 1, turnLimit: 30 }), /SETTINGS > DIGIKIT > MODULES/);
  assert.match(remoteNotice(undefined), /Reading/);
  const noModule = new DkError({ error: 'remote state: E_PROTO', kind: 'device', status: 'E_PROTO' });
  assert.match(remoteNotice(undefined, noModule), /not loaded.*Transfer page/);
  const other = new DkError({ error: 'no reply from the device', kind: 'timeout' });
  assert.equal(remoteNotice(undefined, other), 'no reply from the device');
});

test('the device behind a session', () => {
  assert.equal(deviceOfPort('dt2'), 'dt2');
  assert.equal(deviceOfPort('dn2-1.12'), 'dn2');
  assert.equal(deviceOfPort('other'), undefined);
});

test('the runtime of the faceplate sends one request per key and turn', async () => {
  const calls: (string | number | boolean)[][] = [];
  const session = {
    remoteKey: async (code: number, down: boolean) => calls.push(['key', code, down]),
    remoteTurn: async (enc: number, d: number) => calls.push(['turn', enc, d]),
  };
  const rt = remoteRuntime(async () => session as unknown as DkSession);
  await rt.press(10);
  await rt.release(10);
  await rt.turn(3, -4);
  assert.deepEqual(calls, [
    ['key', 10, true],
    ['key', 10, false],
    ['turn', 3, -4],
  ]);
});

test('once the module is missing, REMOTE_STATE is asked again only when MODULES lists digikit/remote', () => {
  const noModule = new DkError({ error: 'remote state: E_PROTO', kind: 'device', status: 'E_PROTO' });
  assert.equal(isRemoteMissing(noModule), true);
  assert.equal(isRemoteMissing(new DkError({ error: 'no reply', kind: 'timeout' })), false);
  assert.equal(remoteStateWanted(false, undefined), true);
  assert.equal(remoteStateWanted(true, [{ name: 'test/crash' }]), false);
  assert.equal(remoteStateWanted(true, []), false);
  assert.equal(remoteStateWanted(true, [{ name: 'digikit/remote' }]), true);
  assert.equal(remoteStateWanted(true, undefined), true); // the list could not be read: ask as before
});

test('INFO only names the device: asked first, when the target changes, and every 30 s, not every poll', () => {
  assert.equal(infoDue(undefined, 1000, false), true);
  assert.equal(infoDue(1000, 3000, false), false);
  assert.equal(infoDue(1000, 3000, true), true);
  assert.equal(infoDue(1000, 1000 + INFO_EVERY_MS, false), true);
});

test('a Transfer job keeps the Remote page from cancelling its requests and from polling', () => {
  // leaving the page cancels what waits on the session only when no job writes to the device (a read is short and not worth keeping)
  assert.equal(leaveCancels(''), true);
  assert.equal(leaveCancels('Reading'), true);
  for (const job of ['Uploading', 'Installing', 'Deleting', 'Restarting']) {
    assert.equal(leaveCancels(job), false, job);
    assert.equal(remotePaused(job), true, job);
  }
  assert.equal(remotePaused(''), false);
  assert.equal(remotePaused('Reading'), false);
});

test('E_PROTO from the state request is no "not loaded" when MODULES lists digikit/remote (the state request is optional)', () => {
  const noState = new DkError({ error: 'remote state: E_PROTO', kind: 'device', status: 'E_PROTO' });
  assert.equal(remoteListed([{ name: 'digikit/remote' }]), true);
  assert.equal(remoteListed([{ name: 'test/crash' }]), false);
  assert.equal(remoteListed(undefined), undefined);
  assert.equal(remoteNotice(undefined, noState, true), '');
  assert.match(remoteNotice(undefined, noState, false), /not loaded/);
  assert.match(remoteNotice(undefined, noState, undefined), /not loaded/);
});

test('a key without digikit/remote gives a plain lasting line; other errors keep their text', () => {
  const noModule = new DkError({
    error: 'remote key: E_PROTO unknown protocol version',
    kind: 'device',
    status: 'E_PROTO',
  });
  assert.match(
    keyErrorLine(noModule, 'the target'),
    /^Keys need digikit\/remote: install it on the Transfer page.*restart/,
  );
  const silent = new DkError({ error: 'no reply', kind: 'timeout' });
  assert.match(keyErrorLine(silent, 'the target'), /No reply from the target/);
});

test('a key reports its failure, then throws; a key that works reports ok', async () => {
  const seen: string[] = [];
  const noModule = new DkError({ error: 'E_PROTO', kind: 'device', status: 'E_PROTO' });
  let fail = true;
  const session = {
    remoteKey: async () => {
      if (fail) throw noModule;
    },
    remoteTurn: async () => {},
  };
  const rt = remoteRuntime(async () => session as unknown as DkSession, {
    ok: () => seen.push('ok'),
    failed: (e) => seen.push(isRemoteMissing(e) ? 'missing' : 'other'),
  });
  await assert.rejects(rt.press(3));
  fail = false;
  await rt.press(3);
  await rt.turn(1, 1);
  assert.deepEqual(seen, ['missing', 'ok', 'ok']);
});

test('a module listed without the state request is not asked again until the target or the list changes', () => {
  const listed = [{ name: 'digikit/remote' }];
  const key = stateProbeKey('midi A', listed);
  assert.equal(remoteStateWanted(true, listed, key, key), false);
  assert.equal(remoteStateWanted(true, listed, key, stateProbeKey('midi B', listed)), true);
  assert.equal(remoteStateWanted(true, listed, key, stateProbeKey('midi A', [...listed, { name: 'x/y' }])), true);
  assert.equal(stateProbeKey('t', [{ name: 'b' }, { name: 'a' }]), stateProbeKey('t', [{ name: 'a' }, { name: 'b' }]));
  // a reload or an upgrade of the same module asks again
  assert.notEqual(
    stateProbeKey('t', [{ name: 'a', version: 1, cf_base: 16 }]),
    stateProbeKey('t', [{ name: 'a', version: 2, cf_base: 16 }]),
  );
  assert.notEqual(
    stateProbeKey('t', [{ name: 'a', version: 1, cf_base: 16 }]),
    stateProbeKey('t', [{ name: 'a', version: 1, cf_base: 32 }]),
  );
});

test('switching away releases latched keys on the port that received the press', async () => {
  const sent: string[] = [];
  const { target } = createFakeTransferApp({
    sessions: createFakeSessions((key) => ({
      remoteKey: async () => sent.push(key + ' press'),
      remoteRelease: async () => sent.push(key + ' release'),
    })),
  });
  await target.selectTarget({ kind: 'midi', port: 'A' });
  const source = target.bindSession();
  const panel = remoteRuntime(source.session);
  source.beforeLeave(panel.releaseHeld);
  await panel.press(1);
  await target.selectTarget({ kind: 'midi', port: 'B' });
  assert.deepEqual(sent, ['midi:A press', 'midi:A release']);
});

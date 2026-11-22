import assert from 'node:assert/strict';
import { DkSession } from '@digi/dk-session';
import { test } from 'vitest';
import { targetOps } from '../src/target-ops/session-store';
import { createFakeSessions, createFakeTransferApp } from '../src/transfer/fake-app';
import type { Target } from '../src/transfer/target';

for (const origin of [{ kind: 'emulator' }, { kind: 'midi', port: 'A' }] satisfies Target[]) {
  test(`switching from ${origin.kind} discards a pending file read before requesting another chunk`, async () => {
    let started!: () => void;
    const reading = new Promise<void>((resolve) => (started = resolve));
    let finish!: () => void;
    const pending = new Promise<void>((resolve) => (finish = resolve));
    const calls: string[] = [];
    const sessions = createFakeSessions((key) => ({
      async read() {
        calls.push(key);
        started();
        await pending;
        return { size: 2, bytes: Uint8Array.of(1) };
      },
    }));
    const { target } = createFakeTransferApp({ sessions });
    await target.selectTarget(origin);
    const binding = targetOps(target);
    const result = binding.store.readAll('settings');
    await reading;
    await target.selectTarget({ kind: 'midi', port: 'B' });
    finish();
    await assert.rejects(result, /target changed/);
    assert.deepEqual(calls, [origin.kind === 'midi' ? 'midi:A' : 'emulator']);
    await target.selectTarget(origin);
    await assert.rejects(binding.store.verify('settings'), /target changed/);
  });
}

test('only the current emulator selection can reset the native runtime', async () => {
  const { target } = createFakeTransferApp();
  let resets = 0;
  const actions = { reset: async () => void resets++ };
  const emulator = targetOps(target, actions);
  await emulator.run.reset!();
  await target.selectTarget({ kind: 'midi', port: 'B' });
  assert.equal(targetOps(target, actions).run.reset, undefined);
  await assert.rejects(emulator.run.reset!(), /target changed/);
  assert.equal(resets, 1);
});

test('a background MIDI session cannot reclaim the host slot while the next port opens', async () => {
  const opened: string[] = [];
  let finish!: () => void;
  const opening = new Promise<void>((resolve) => (finish = resolve));
  const { target } = createFakeTransferApp({
    env: {
      targets: {
        emulator: true,
        ports: async () => ['A', 'B'],
        open: async (t) => {
          if (t.kind !== 'midi') throw new Error('Expected a MIDI port');
          opened.push(t.port);
          if (t.port === 'B') await opening;
          return { send() {}, recv: async () => null };
        },
      },
    },
  });
  await target.selectTarget({ kind: 'midi', port: 'A' });
  const switching = target.selectTarget({ kind: 'midi', port: 'B' });
  await assert.rejects(target.lastMidiSession(), /port changed/);
  finish();
  await switching;
  await target.session();
  assert.deepEqual(opened, ['A', 'B']);
});

test('the raw firmware transport waits for an outstanding session read before taking the MIDI port', async () => {
  let receiving!: () => void;
  const started = new Promise<void>((resolve) => {
    receiving = resolve;
  });
  let reply!: () => void;
  const waiting = new Promise<null>((resolve) => {
    reply = () => resolve(null);
  });
  const link = {
    send() {},
    recv: () => {
      receiving();
      return waiting;
    },
  };
  const { target } = createFakeTransferApp({
    env: { targets: { emulator: true, ports: async () => ['A'], open: async () => link } },
    sessions: createFakeSessions((_key, transport) => new DkSession(transport, { timeoutMs: 1, retries: 0 })),
  });
  await target.selectTarget({ kind: 'midi', port: 'A' });
  const session = await target.session();
  const reading = session.logPage(0, true).catch(() => {});
  await started;
  const order: string[] = [];
  const raw = target.portTransport().then((port) => {
    order.push('handoff');
    return port;
  });
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  order.push('reply');
  reply();
  await reading;
  assert.equal(await raw, link);
  await assert.rejects(session.logPage(0, true), { kind: 'cancelled' });
  await assert.rejects(session.screen(), { kind: 'cancelled' });
  target.finishRawTransport();
  assert.deepEqual(order, ['reply', 'handoff']);
});

test('a background runtime INFO timeout preserves the identity of a confirmed stock device', async () => {
  const app = createFakeTransferApp({
    sessions: createFakeSessions((_key, transport, options) => new DkSession(transport, options)),
  });
  await app.target.selectTarget({ kind: 'midi', port: 'A' });
  const session = await app.target.session();
  app.connection.noteConnection({ type: 'noruntime', reason: 'Stock firmware' });
  await assert.rejects(session.info(1), { kind: 'timeout' });
  assert.equal(app.connection.connection().state, 'noruntime');
  app.connection.noteConnection({ type: 'lost', reason: 'Stock screenshot failed' });
  assert.equal(app.connection.connection().state, 'disconnected');
});

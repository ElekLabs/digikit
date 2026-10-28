// One session per target (apps/web/src/transfer/session-core.ts): every page and panel that talks to the emulator or to a MIDI port gets the SAME
// DkSession, so their requests take turns on the one reply stream instead of stealing each other's replies.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, DkSession, frame } from '@digi/dk-session';
import { createSessions, targetKey } from '../src/transfer/session-core.ts';

/** A device behind one reply stream, like the emulator's USB capture: any session that polls takes the next reply. It answers a request after a
 * moment, and counts how many requests were on the wire at once (sent, not yet answered). */
class Wire {
  replies: Uint8Array[];
  open: number;
  maxOpen: number;
  sent: number;
  constructor() {
    this.replies = [];
    this.open = 0;
    this.maxOpen = 0;
    this.sent = 0;
  }
  async send(msg: Uint8Array) {
    this.sent++;
    this.open++;
    this.maxOpen = Math.max(this.maxOpen, this.open);
    const cmd = msg[5];
    const seq = msg[6];
    setTimeout(() => {
      this.open--;
      // the reply carries the command and the request's seq, and the body echoes the cmd
      this.replies.push(frame(cmd | 0x40, seq, Uint8Array.of(0, cmd)));
    }, 15);
  }
  async recv(timeoutMs: number) {
    const end = Date.now() + timeoutMs;
    for (;;) {
      const next = this.replies.shift();
      if (next) return next;
      if (Date.now() >= end) return null;
      await new Promise((r) => setTimeout(r, 2));
    }
  }
}

test('the emulator has one key, whichever page asks; a MIDI port has its own', () => {
  assert.equal(targetKey({ kind: 'emulator' }), 'emulator');
  assert.equal(targetKey({ kind: 'midi', port: 'Elektron Digitakt II' }), 'midi:Elektron Digitakt II');
  assert.notEqual(targetKey({ kind: 'midi', port: 'a' }), targetKey({ kind: 'midi', port: 'b' }));
});

test('two pages get the same session, made once', () => {
  let made = 0;
  const sessions = createSessions(() => {
    made++;
    return new DkSession(new Wire(), { timeoutMs: 1000 });
  });
  const transfer = sessions.get(targetKey({ kind: 'emulator' })); // the Transfer page
  const memory = sessions.get(targetKey({ kind: 'emulator' })); // the Memory panel
  assert.equal(transfer, memory);
  assert.equal(made, 1);
  // another link is another session; a dropped one is made again
  assert.notEqual(sessions.get('midi:x'), transfer);
  sessions.drop('midi:x');
  assert.equal(made, 2);
  sessions.get('midi:x');
  assert.equal(made, 3);
});

test('requests from two pages on the shared session go one at a time and each gets its own reply', async () => {
  const wire = new Wire();
  const sessions = createSessions(() => new DkSession(wire, { timeoutMs: 2000 }));
  const page = () => sessions.get('emulator');
  const [a, b, c, d] = await Promise.all([
    page().request(0x11), // the Transfer page's INFO
    page().request(0x12), // the Memory panel's CAPACITY
    page().request(0x13),
    page().request(0x14),
  ]);
  assert.deepEqual(
    [a, b, c, d].map((r) => r.body[0]),
    [0x11, 0x12, 0x13, 0x14],
  );
  assert.equal(wire.sent, 4, 'no request was sent twice');
  assert.equal(wire.maxOpen, 1, 'a request waited for the one before it');
});

test('cancel reaches only the session of that target, and a target without a session has nothing to cancel', async () => {
  const silent = {
    send() {},
    recv: (ms: number, signal?: AbortSignal) =>
      new Promise<Uint8Array | null>((res) => {
        const t = setTimeout(() => res(null), ms);
        signal?.addEventListener('abort', () => (clearTimeout(t), res(null)), { once: true });
      }),
  };
  const sessions = createSessions(() => new DkSession(silent, { timeoutMs: 30_000 }));
  const emu = sessions.get('emulator');
  const midi = sessions.get('midi:x');
  const waiting = emu.request(0x11);
  const other = midi.request(0x11);
  await new Promise((r) => setTimeout(r, 20));
  sessions.cancel('midi:x');
  sessions.cancel('midi:none');
  await assert.rejects(other, (e: DkError) => e.kind === 'cancelled');
  sessions.cancel('emulator');
  await assert.rejects(waiting, (e: DkError) => e.kind === 'cancelled');
});

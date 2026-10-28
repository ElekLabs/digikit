// A flash from the first message to the verdict (apps/web/src/firmware/firmware-flow.ts), over a fake device on a virtual clock. The pacing is
// the Rust sender's (patchkit.wasm); the tests skip when the wasm is not built.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { test } from 'vitest';
import { PatchKit, type PreparedFlash } from '../src/patch/patchkit-core.ts';
import type { Answer, FlashState } from '../src/firmware/firmware-core.ts';
import { runFlash, type FlowLimits, type FlowOps } from '../src/firmware/firmware-flow.ts';

const wasm = fileURLToPath(new URL('../public/patchkit.wasm', import.meta.url));
const have = existsSync(wasm);
const kit = have ? await PatchKit.create(new Uint8Array(readFileSync(wasm)), '{"ports":[],"modules":[]}') : undefined;

const TOTAL = 40;
function prepared(over: Partial<PreparedFlash> = {}): PreparedFlash {
  const image = Uint8Array.from({ length: TOTAL * 8 }, (_, i) => i & 0xff);
  return {
    ok: true,
    restore: false,
    device: 'dn2',
    os: '1.12',
    shaStock: 'a'.repeat(64),
    shaImage: 'b'.repeat(64),
    imageId: 'c5d8ccb27e139cbc',
    runtime: '0.5.2',
    paceMs: 2,
    checks: [],
    ends: Array.from({ length: TOTAL }, (_, i) => (i + 1) * 8),
    image,
    ...over,
  };
}

const SHORT: FlowLimits = { leaveMs: 3000, returnMs: 3000, askMs: 6000, pollMs: 100, askEveryMs: 500 };
const GOOD: Answer = { os: '1.12', runtime: '0.5.2', imageId: 'c5d8ccb27e139cbc' };

/** A device on a virtual clock: its port goes away `goneAfter` ms after the last message and is back `backAfter` ms later. */
function fake(opts: {
  goneAfter?: number | null;
  backAfter?: number | null;
  answers?: (n: number) => Answer | Error;
  failAt?: number;
}) {
  const t = { ms: 1000 };
  const sent: { at: number; msg: Uint8Array }[] = [];
  let lastSend = 0;
  let released = false;
  let asks = 0;
  const ops: FlowOps = {
    sender: async (total) => kit!.sender(total),
    send(m) {
      if (opts.failAt !== undefined && sent.length === opts.failAt) throw new Error('The MIDI port is gone');
      sent.push({ at: t.ms, msg: m });
      lastSend = t.ms;
    },
    now: () => t.ms,
    sleep: async (ms) => {
      t.ms += ms;
    },
    release: async () => {
      released = true;
    },
    portPresent: async () => {
      if (!released) return true;
      const gone = opts.goneAfter === undefined ? 500 : opts.goneAfter;
      const back = opts.backAfter === undefined ? 1500 : opts.backAfter;
      if (gone === null) return true;
      const since = t.ms - lastSend;
      if (since < gone) return true;
      if (back === null) return false;
      return since >= gone + back;
    },
    async ask() {
      const a = (opts.answers ?? (() => GOOD))(asks++);
      if (a instanceof Error) throw a;
      return a;
    },
  };
  return { ops, sent, t, wasReleased: () => released, asks: () => asks };
}

const run = (
  f: ReturnType<typeof fake>,
  p: PreparedFlash,
  signal = new AbortController().signal,
  states: FlashState[] = [],
) => runFlash(f.ops, p, signal, (s) => states.push(s), SHORT);
const code = (s: FlashState) => (s.phase === 'failed' ? s.code : s.phase);
const message = (s: FlashState) => ('message' in s ? s.message : '');

test(
  'a patch is sent in order, 2 ms apart, then the restart and the runtime are checked',
  { skip: !have },
  async () => {
    const p = prepared();
    const f = fake({});
    const states: FlashState[] = [];
    const end = await run(f, p, undefined, states);
    assert.equal(end.phase, 'done');
    assert.match(message(end), /runtime 0\.5\.2 · image id c5d8ccb27e139cbc matches/);
    assert.equal(f.sent.length, TOTAL);
    // the messages are the image's, in order
    assert.deepEqual(Buffer.concat(f.sent.map((s) => s.msg)), Buffer.from(p.image));
    for (let i = 1; i < f.sent.length; i++)
      assert.ok(f.sent[i]!.at - f.sent[i - 1]!.at >= 2, `message ${i} comes 2 ms after the one before`);
    assert.ok(f.wasReleased());
    // the states go sending, rebooting (leaving, returning, asking), done
    const phases = [...new Set(states.map((s) => (s.phase === 'rebooting' ? `rebooting:${s.stage}` : s.phase)))];
    assert.deepEqual(phases, ['sending', 'rebooting:leaving', 'rebooting:returning', 'rebooting:asking', 'done']);
    const last = states.filter((s) => s.phase === 'sending').at(-1);
    assert.deepEqual(last?.phase === 'sending' && [last.sent, last.total], [TOTAL, TOTAL]);
  },
);

test(
  'Cancel stops the sending at once; the device keeps its image and the port is not released',
  { skip: !have },
  async () => {
    const f = fake({});
    const ctl = new AbortController();
    const origSend = f.ops.send;
    f.ops.send = (m) => {
      void origSend(m);
      if (f.sent.length === 7) ctl.abort();
    };
    const end = await run(f, prepared(), ctl.signal);
    assert.deepEqual(end.phase === 'stopped' && [end.reason, end.sent, end.total], ['cancelled', 7, TOTAL]);
    assert.match(message(end), /Press NO on the device to cancel, or send again from the start/);
    assert.equal(f.sent.length, 7);
    assert.equal(f.wasReleased(), false);
  },
);

test('a port that fails mid-send stops the run, and the sender does not go on', { skip: !have }, async () => {
  const f = fake({ failAt: 12 });
  const end = await run(f, prepared());
  assert.deepEqual(end.phase === 'stopped' && [end.reason, end.sent], ['error', 12]);
  assert.match(message(end), /Message 13 of 40 failed: The MIDI port is gone\. The device has changed nothing/);
  assert.equal(f.sent.length, 12);
  assert.equal(f.wasReleased(), false);
});

test(
  'a device that does not restart, or does not come back, is told apart with a clear message',
  { skip: !have },
  async () => {
    const never = await run(fake({ goneAfter: null }), prepared());
    assert.equal(code(never), 'no-restart');
    assert.match(message(never), /did not restart in time.*press NO and send again/s);
    const gone = await run(fake({ backAfter: null }), prepared());
    assert.equal(code(gone), 'no-return');
    assert.match(message(gone), /did not come back in time/);
  },
);

test('the runtime or the image id of another image is a failure', { skip: !have }, async () => {
  assert.equal(code(await run(fake({ answers: () => ({ ...GOOD, runtime: '0.5.1' }) }), prepared())), 'runtime');
  assert.equal(
    code(await run(fake({ answers: () => ({ ...GOOD, imageId: 'ffffffffffffffff' }) }), prepared())),
    'image-id',
  );
});

test('a device that answers late is waited for; one that never does is a failure', { skip: !have }, async () => {
  const late = fake({ answers: (n) => (n < 3 ? new Error('no reply') : GOOD) });
  assert.equal((await run(late, prepared())).phase, 'done');
  assert.equal(late.asks(), 4);
  // the stock part answers, the runtime does not: wait for it, then say so
  assert.equal(code(await run(fake({ answers: () => ({ os: '1.12' }) }), prepared())), 'runtime');
  assert.equal(code(await run(fake({ answers: () => new Error('no reply') }), prepared())), 'no-answer');
});

test('a restore ends when the stock firmware answers twice without a runtime', { skip: !have }, async () => {
  const p = prepared({ restore: true, runtime: null, imageId: null });
  const f = fake({ answers: () => ({ os: '1.12' }) });
  const end = await run(f, p);
  assert.equal(end.phase, 'done');
  assert.equal(f.asks(), 2);
  assert.match(message(end), /stock OS 1\.12, no runtime/);
  // a runtime that is still there is a failure at once
  assert.equal(code(await run(fake({ answers: () => GOOD }), p)), 'still-runtime');
});

test('the user can stop waiting for the restart', { skip: !have }, async () => {
  const ctl = new AbortController();
  const f = fake({ goneAfter: null });
  const origSleep = f.ops.sleep;
  f.ops.sleep = async (ms) => {
    await origSleep(ms);
    if (f.wasReleased()) ctl.abort();
  };
  const end = await run(f, prepared(), ctl.signal);
  assert.equal(code(end), 'no-restart');
  assert.match(message(end), /You stopped waiting/);
});

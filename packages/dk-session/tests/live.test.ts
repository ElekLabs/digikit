// LOAD, UNLOAD and RELOAD of a content module (the runtime's one live job) in the pure TypeScript client (`DkSession`) against a fake device that frames its replies byte for byte.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, DkSession, decode8in7, encode8in7 } from '@digi/dk-session';
import { CMD } from '@digi/dk-session/gen_proto';

const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const PREFIX = [0xf0, 0x7d, 0x64, 0x6b, 1];
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from(PREFIX),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );
const i32 = (n: number) => [(n >> 24) & 255, (n >> 16) & 255, (n >> 8) & 255, n & 255];

/** A device whose live job needs `busyPolls` asks of RELOAD_STATE to end with `result`; a request it answers with `queueStatus` is not queued. */
class FakeLive {
  seen: { cmd: number; path: string }[] = [];
  queue: Uint8Array[] = [];
  polls = 0;
  running = false;
  op = 0;
  busyPolls: number;
  result: number;
  unloaded: number;
  queueStatus: number;
  constructor(busyPolls = 2, result = 0, unloaded = 0, queueStatus = 0) {
    this.busyPolls = busyPolls;
    this.result = result;
    this.unloaded = unloaded;
    this.queueStatus = queueStatus;
  }
  send(msg: Uint8Array) {
    const cmd = msg[5]!;
    const seq = msg[6]!;
    const body = decode8in7(msg.subarray(7, msg.length - 1));
    if (cmd === CMD.RELOAD_STATE) {
      this.polls++;
      const done = this.running && this.polls > this.busyPolls;
      const phase = done ? 2 : this.running ? 1 : 0;
      return void this.queue.push(
        reply(cmd, seq, 0, [phase, ...i32(done ? this.result : 0), done ? 6 : 1, done ? this.unloaded : 0, this.op]),
      );
    }
    this.seen.push({ cmd, path: new TextDecoder().decode(body.subarray(0, body.length - 1)) });
    if (body[body.length - 1] !== 0) return void this.queue.push(reply(cmd, seq, -60));
    if (this.queueStatus === 0) {
      this.op = cmd === CMD.LOAD ? 1 : cmd === CMD.UNLOAD ? 2 : 3;
      this.running = true;
      this.polls = 0;
    }
    this.queue.push(reply(cmd, seq, this.queueStatus));
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

test('the new commands have numbers of their own next to RELOAD', () => {
  assert.deepEqual([CMD.RELOAD, CMD.RELOAD_STATE, CMD.LOAD, CMD.UNLOAD], [0x06, 0x07, 0x08, 0x09]);
});

test('each operation sends its command with the NUL-terminated path, then asks the one state command until the job ends', async () => {
  for (const [op, cmd] of [
    ['loadAndWait', CMD.LOAD],
    ['unloadAndWait', CMD.UNLOAD],
    ['reloadAndWait', CMD.RELOAD],
  ] as const) {
    const dev = new FakeLive(2, 0, op === 'loadAndWait' ? 0 : 1);
    const s = new DkSession(dev, { timeoutMs: 200 });
    const end = await s[op]('modules/a.dkm', { pollMs: 0 });
    assert.deepEqual(dev.seen, [{ cmd, path: 'modules/a.dkm' }], op);
    assert.deepEqual(end, { result: 0, unloaded: op !== 'loadAndWait' }, op);
    assert.equal(dev.polls, 3, `${op}: two asks while it runs, one that sees it finished`);
    assert.equal(
      (await s.reloadState()).op,
      op === 'loadAndWait' ? 1 : op === 'unloadAndWait' ? 2 : 3,
      `${op}: the state says which job it is`,
    );
  }
});

test('a refusal before anything changed and a failure after the teardown come back as numbers, a refusal at the queue as an error with its status', async () => {
  const s = new DkSession(new FakeLive(0, -83, 0), { timeoutMs: 200 });
  assert.deepEqual(await s.unloadAndWait('modules/a.dkm', { pollMs: 0 }), { result: -83, unloaded: false });
  const failed = new DkSession(new FakeLive(0, -9, 1), { timeoutMs: 200 });
  assert.deepEqual(await failed.reloadAndWait('modules/a.dkm', { pollMs: 0 }), { result: -9, unloaded: true });
  // E_LOADED (-10) answers a LOAD of a path that is loaded
  const loaded = new DkSession(new FakeLive(0, 0, 0, -10), { timeoutMs: 200 });
  await assert.rejects(
    loaded.loadAndWait('modules/a.dkm', { pollMs: 0 }),
    (e) => e instanceof DkError && e.status === 'E_LOADED',
  );
});

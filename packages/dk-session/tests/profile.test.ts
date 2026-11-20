// PROFILE_STATE and PROFILE_READ in the pure TypeScript client (`DkSession.profileState`, `profileRead`, `profileAll`) against a fake device
// that answers from a queue of scripted replies and records the request bodies.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, DkSession, decode8in7, encode8in7 } from '@digi/dk-session';
import { SERVICE_CMD } from '@digi/dk-session/gen_proto';

const PREFIX = [0xf0, 0x7d, 0x64, 0x6b, 1];
const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from(PREFIX),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );

const be = (v: bigint | number, bytes: number) =>
  Array.from({ length: bytes }, (_, i) => Number((BigInt(v) >> BigInt(8 * (bytes - 1 - i))) & 0xffn));
const row = (calls: number, sum: bigint, max: number) => [...be(calls, 4), ...be(sum, 8), ...be(max, 4)];

class FakeDebug {
  requests: { cmd: number; body: number[] }[] = [];
  queue: Uint8Array[] = [];
  replies: { status?: number; body?: number[] }[];
  constructor(replies: { status?: number; body?: number[] }[]) {
    this.replies = replies;
  }
  send(msg: Uint8Array) {
    this.requests.push({ cmd: msg[5]!, body: [...decode8in7(msg.subarray(7, msg.length - 1))] });
    const r = this.replies.shift() ?? { status: -1 };
    this.queue.push(reply(msg[5]!, msg[6]!, r.status ?? 0, r.body ?? []));
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

const session = (replies: { status?: number; body?: number[] }[]) => {
  const dev = new FakeDebug(replies);
  return { dev, s: new DkSession(dev, { timeoutMs: 200 }) };
};

test('profileState sends the op byte and decodes state, clock and rows', async () => {
  const ops = { get: 0, start: 1, stop: 2, reset: 3 } as const;
  for (const [op, code] of Object.entries(ops)) {
    const { dev, s } = session([{ body: [1, ...be(120_000_000, 4), ...be(15, 2), ...be(2n ** 40n + 5n, 8)] }]);
    const st = await s.profileState(op as keyof typeof ops);
    assert.deepEqual(st, { running: true, hz: 120_000_000, rows: 15, elapsedTicks: 2n ** 40n + 5n, polls: [] });
    assert.deepEqual(dev.requests, [{ cmd: SERVICE_CMD.PROFILE_STATE, body: [code] }]);
  }
  const { s } = session([{ body: [0, ...be(1000, 4), ...be(7, 2), ...be(0, 8)] }]);
  assert.equal((await s.profileState('get')).running, false);
});

test('profileState decodes the poll slots: empty, a module record and the firmware image', async () => {
  const polls = [0, ...be(0, 4), 3, ...be(0x47e10040, 4), 0xff, ...be(0x40101000, 4), 1, ...be(0x47e00010, 4)];
  const { s } = session([{ body: [1, ...be(1000, 4), ...be(15, 2), ...be(5, 8), ...polls] }]);
  assert.deepEqual((await s.profileState('get')).polls, [
    undefined,
    { owner: 2, fn: 0x47e10040 },
    { owner: 'runtime', fn: 0x40101000 },
    { owner: 0, fn: 0x47e00010 },
  ]);
  const old = session([{ body: [1, ...be(1000, 4), ...be(15, 2), ...be(5, 8)] }]);
  assert.deepEqual((await old.s.profileState('get')).polls, [], 'a debug module without the slots');
});

test('profileRead sends the first row and decodes rows with a u64 sum', async () => {
  const sum = 0x1_2345_6789n;
  const { dev, s } = session([{ body: [...be(300, 2), 2, ...row(5, sum, 9), ...row(1, 2n ** 40n, 0xffffffff)] }]);
  const r = await s.profileRead(300);
  assert.deepEqual(dev.requests, [{ cmd: SERVICE_CMD.PROFILE_READ, body: [1, 44] }]);
  assert.equal(r.first, 300);
  assert.deepEqual(r.rows, [
    { calls: 5, sumTicks: sum, maxTicks: 9 },
    { calls: 1, sumTicks: 2n ** 40n, maxTicks: 0xffffffff },
  ]);
});

test('profileAll pages over two READ replies and maps row kinds', async () => {
  const rows = Array.from({ length: 15 }, (_, i) => row(i + 1, BigInt(i), i));
  const { dev, s } = session([
    { body: [1, ...be(1000, 4), ...be(15, 2), ...be(777, 8)] },
    { body: [...be(0, 2), 10, ...rows.slice(0, 10).flat()] },
    { body: [...be(10, 2), 5, ...rows.slice(10).flat()] },
  ]);
  const all = await s.profileAll();
  assert.deepEqual(
    dev.requests.map((r) => [r.cmd, r.body]),
    [
      [SERVICE_CMD.PROFILE_STATE, [0]],
      [SERVICE_CMD.PROFILE_READ, [0, 0]],
      [SERVICE_CMD.PROFILE_READ, [0, 10]],
    ],
  );
  assert.equal(all.running, true);
  assert.equal(all.hz, 1000);
  assert.equal(all.elapsedTicks, 777n);
  assert.deepEqual(
    all.spans.map((x) => [x.row, x.kind, x.index, x.calls]),
    [
      ...[0, 1, 2, 3, 4, 5, 6, 7].map((i) => [i, 'module', i, i + 1]),
      ...[0, 1, 2].map((i) => [8 + i, 'phase', i, 9 + i]),
      ...[0, 1, 2, 3].map((i) => [11 + i, 'poll', i, 12 + i]),
    ],
  );
});

test('a short reply is a protocol error', async () => {
  await assert.rejects(
    session([{ body: [1, 0, 0] }]).s.profileState('get'),
    (e) => e instanceof DkError && e.kind === 'protocol',
  );
  // the 7-byte reply of a debug module without the elapsed window
  await assert.rejects(
    session([{ body: [1, ...be(1000, 4), ...be(7, 2)] }]).s.profileState('get'),
    (e) => e instanceof DkError && e.kind === 'protocol',
  );
  await assert.rejects(
    session([{ body: [...be(0, 2), 2, ...row(1, 1n, 1)] }]).s.profileRead(0),
    (e) => e instanceof DkError && e.kind === 'protocol',
  );
});

test('an error status is a DkError', async () => {
  await assert.rejects(session([{ status: -63 }]).s.profileState('start'), DkError);
  await assert.rejects(session([{ status: -63 }]).s.profileRead(0), DkError);
});

// PROFILE_DSP: header (version, flags, generation, rows, sweep number, three counters), then the 100 words of a sweep when flag bit 1 is set.
const dspHead = (flags: number, gen: number, sweep: number) => [
  1,
  flags,
  gen,
  16,
  ...be(sweep, 4),
  ...be(11, 4),
  ...be(2, 4),
  ...be(5, 4),
];
const dspWindow = () => {
  const w = Array.from({ length: 100 }, () => 0);
  w[0] = 1500; // blocks
  w[1] = 666_000; // largest stock block delta
  w[2] = 3; // overruns
  // row 9 (an effect): 7 calls, 2^32 + 5 cycles, largest call 900, largest block 4000
  w.splice(4 + 6 * 9, 5, 7, 5, 1, 900, 4000);
  return w.flatMap((x) => be(x, 4));
};

test('profileDsp sends the op byte and decodes the header, counters and rows', async () => {
  for (const [op, code] of Object.entries({ get: 0, start: 1, stop: 2, clear: 3 })) {
    const { dev, s } = session([{ body: [...dspHead(3, 4, 12), ...dspWindow()] }]);
    const d = await s.profileDsp(op as 'get');
    assert.deepEqual(dev.requests, [{ cmd: SERVICE_CMD.PROFILE_DSP, body: [code] }]);
    assert.equal(d.on, true);
    assert.equal(d.generation, 4);
    assert.deepEqual(d.counts, { sweeps: 11, gaps: 2, ignored: 5 });
    assert.deepEqual(
      [d.sweep!.number, d.sweep!.blocks, d.sweep!.maxStockBlockDelta, d.sweep!.overruns],
      [12, 1500, 666_000, 3],
    );
    assert.equal(d.sweep!.rows.length, 16);
    assert.deepEqual(d.sweep!.rows[9], {
      row: 9,
      kind: 'effect',
      calls: 7,
      sumCycles: 2n ** 32n + 5n,
      maxCall: 900,
      maxBlock: 4000,
    });
    assert.equal(d.sweep!.rows[0]!.kind, 'machine');
  }
});

test('profileDsp without a complete sweep has counters only', async () => {
  const { s } = session([{ body: dspHead(0, 0, 0) }]);
  assert.deepEqual(await s.profileDsp(), { on: false, generation: 0, counts: { sweeps: 11, gaps: 2, ignored: 5 } });
});

test('profileDsp refuses a short or foreign reply', async () => {
  const bad = (body: number[]) =>
    assert.rejects(session([{ body }]).s.profileDsp(), (e) => e instanceof DkError && e.kind === 'protocol');
  await bad([1, 0, 0]);
  await bad(dspHead(3, 0, 1)); // a sweep flagged but not sent
  await bad([2, ...dspHead(0, 0, 0).slice(1)]); // another version
});

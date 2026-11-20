// SETUP in the pure TypeScript client against a fake debug module: the request is op, track, type; the wait is on SETUP_DONE with the request's seq.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, DkSession, SetupRefused, decode8in7, encode8in7, runtimeEvent, trackSetup } from '@digi/dk-session';
import { LOG_KIND, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const PREFIX = [0xf0, 0x7d, 0x64, 0x6b, 1];
const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from(PREFIX),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );

class FakeDebug {
  bodies: number[][] = [];
  queue: Uint8Array[] = [];
  busy = false;
  send(msg: Uint8Array) {
    assert.equal(msg[5], SERVICE_CMD.SETUP);
    this.bodies.push([...decode8in7(msg.subarray(7, msg.length - 1))]);
    this.queue.push(this.busy ? reply(msg[5]!, msg[6]!, -12) : reply(msg[5]!, msg[6]!, 0, [this.bodies.length]));
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

test('setup sends op, track and type and returns the seq of the reply', async () => {
  const dev = new FakeDebug();
  const s = new DkSession(dev, { timeoutMs: 200 });
  assert.equal(await s.setup('machine', 3, 5), 1);
  assert.equal(await s.setup('filter', 15, 2), 2);
  assert.deepEqual(dev.bodies, [
    [1, 3, 5],
    [2, 15, 2],
  ]);
});

test('a queued request is refused by the module with its error', async () => {
  const dev = new FakeDebug();
  dev.busy = true;
  await assert.rejects(new DkSession(dev, { timeoutMs: 200 }).setup('machine', 0, 1));
});

const done = (seq: number, status: number) =>
  runtimeEvent({ seq: 1, time: 0, kind: LOG_KIND.SETUP_DONE, flags: 2, args: [1, 3, 5, seq, status >>> 0] });

test('trackSetup waits for the SETUP_DONE of its own seq and throws on a status that is not 0', async () => {
  const dev = new FakeDebug();
  const waits: number[] = [];
  const events = {
    until: async (kind: string, match: (e: ReturnType<typeof done>) => boolean) => {
      assert.equal(kind, 'SETUP_DONE');
      assert.equal(match(done(9, 0)), false, 'another seq is not ours');
      const next = [done(1, 0), done(2, -53)][waits.push(1) - 1]!;
      assert.equal(match(next), true);
      return next;
    },
  };
  const t = trackSetup(new DkSession(dev, { timeoutMs: 200 }), events as never);
  assert.deepEqual(await t.setMachine(3, 5), { seq: 1, status: 0 });
  await assert.rejects(t.setFilter(3, 5), (e: unknown) => e instanceof SetupRefused && e.status === -53);
});

test('with a clock the request is repeated while the module answers E_STATE (its track watch has no baseline yet), then taken', async () => {
  let refused = 3;
  const pauses: number[] = [];
  let now = 0;
  const clock = {
    now: async () => now,
    pause: async (ms: number) => {
      pauses.push(ms);
      now += ms;
    },
  };
  const dk = {
    setup: async () => {
      if (refused-- > 0) throw new DkError({ error: 'setup: E_STATE', kind: 'device', status: 'E_STATE' });
      return 4;
    },
  };
  const events = { until: async () => done(4, 0) };
  const t = trackSetup(dk as never, events as never, { ready: { clock, withinMs: 1000 } });
  assert.deepEqual(await t.setMachine(0, 1), { seq: 4, status: 0 });
  assert.deepEqual(pauses, [100, 100, 100]);
  // another error is not retried, and the time limit ends the retries
  const other = { setup: async () => Promise.reject(new DkError({ error: 'busy', kind: 'device', status: 'E_BUSY' })) };
  await assert.rejects(
    trackSetup(other as never, events as never, { ready: { clock, withinMs: 1000 } }).setMachine(0, 1),
    /busy/,
  );
  refused = 1000;
  await assert.rejects(t.setMachine(0, 1), /E_STATE/);
});

// The session recovers a stale link between tries: after a failed try it calls the transport's `recover`, and it reports each request's outcome
// through `onLink`. A device error is an answer: the link works, nothing is recovered.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkSession, encode8in7 } from '../src/index.ts';
import { SERVICE_CMD } from '../src/gen_proto.ts';

const CMD_LIST = 0x02;
const E_FULL = -2;
const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const reply = (cmd: number, seq: number, status: number) =>
  concat(
    Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255)),
    Uint8Array.of(0xf7),
  );

/** A device that answers every request with `status`, but only while the link is alive; `recover` revives the link when `canRevive` is set. */
class StaleLink {
  alive: boolean;
  canRevive: boolean;
  recovers = 0;
  status = 0;
  throwOnRecover = false;
  private queue: Uint8Array[] = [];
  constructor(alive: boolean, canRevive: boolean) {
    this.alive = alive;
    this.canRevive = canRevive;
  }
  send(msg: Uint8Array) {
    if (this.alive) this.queue.push(reply(msg[5]!, msg[6]!, this.status));
  }
  async recv(): Promise<Uint8Array | null> {
    return this.queue.shift() ?? null;
  }
  async recover() {
    this.recovers++;
    if (this.throwOnRecover) throw new Error('not back yet');
    if (this.canRevive) this.alive = true;
  }
}

const listBody = new Uint8Array(4);

test('a dead link is recovered after the failed try, and the request succeeds on the next one', async () => {
  const link = new StaleLink(false, true);
  const outcomes: [boolean, string | undefined][] = [];
  const s = new DkSession(link, { timeoutMs: 20, retries: 2, onLink: (ok, why) => outcomes.push([ok, why]) });
  const r = await s.request(CMD_LIST, listBody);
  assert.equal(r.status, 0);
  assert.equal(link.recovers, 1);
  assert.deepEqual(outcomes, [[true, undefined]]);
});

test('a recover that throws does not stop the retries: the request ends as a timeout and onLink reports it once', async () => {
  const link = new StaleLink(false, false);
  link.throwOnRecover = true;
  const outcomes: [boolean, string | undefined][] = [];
  const s = new DkSession(link, { timeoutMs: 20, retries: 2, onLink: (ok, why) => outcomes.push([ok, why]) });
  await assert.rejects(s.request(CMD_LIST, listBody), { kind: 'timeout' });
  // one recover after each of the three tries, so the next request starts on a fresh link
  assert.equal(link.recovers, 3);
  assert.equal(outcomes.length, 1);
  assert.equal(outcomes[0]![0], false);
  assert.match(outcomes[0]![1]!, /no reply/);
});

test('a device error is an answer: the link is not recovered and onLink says it works', async () => {
  const link = new StaleLink(true, true);
  link.status = E_FULL;
  const outcomes: boolean[] = [];
  const s = new DkSession(link, { timeoutMs: 20, retries: 2, onLink: (ok) => outcomes.push(ok) });
  const r = await s.request(CMD_LIST, listBody);
  assert.notEqual(r.status, 0);
  assert.equal(link.recovers, 0);
  assert.deepEqual(outcomes, [true]);
});

test('a transport without recover whose recv throws fails at once, without a retry', async () => {
  let sends = 0;
  const t = {
    send() {
      sends++;
    },
    async recv(): Promise<Uint8Array | null> {
      throw new Error('read failed');
    },
  };
  const s = new DkSession(t, { timeoutMs: 20, retries: 2 });
  await assert.rejects(s.request(CMD_LIST, listBody), /read failed/);
  assert.equal(sends, 1);
});

test('a transport that throws is recovered and its error is rethrown after the last try', async () => {
  const link = new StaleLink(true, false);
  link.send = () => {
    throw new Error('The MIDI port is closed');
  };
  const outcomes: [boolean, string | undefined][] = [];
  const s = new DkSession(link, { timeoutMs: 20, retries: 1, onLink: (ok, why) => outcomes.push([ok, why]) });
  await assert.rejects(s.request(CMD_LIST, listBody), /MIDI port is closed/);
  assert.equal(link.recovers, 2);
  assert.deepEqual(outcomes, [[false, 'The MIDI port is closed']]);
});

test('onLink marks a lost service command (LOGCFG) optional and a core command (LIST) not', async () => {
  const optional: (boolean | undefined)[] = [];
  const s = new DkSession(new StaleLink(false, false), {
    timeoutMs: 20,
    retries: 0,
    onLink: (ok, _why, _silent, opt) => {
      if (!ok) optional.push(opt);
    },
  });
  await assert.rejects(s.request(SERVICE_CMD.LOGCFG, Uint8Array.of(0)), { kind: 'timeout' });
  await assert.rejects(s.request(CMD_LIST, listBody), { kind: 'timeout' });
  assert.deepEqual(optional, [true, false]);
});

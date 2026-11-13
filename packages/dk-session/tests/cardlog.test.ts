// The card log: LOGCFG op 2 and 3 against a fake debug module, and the file decoder on a hand-built file.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, DkSession, decode8in7, encode8in7, parseCardLog } from '@digi/dk-session';
import { LOG_KIND, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from([0xf0, 0x7d, 0x64, 0x6b, 1]),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );

class FakeRuntime {
  cardLog = true;
  on = 0;
  bodies: number[][] = [];
  queue: Uint8Array[] = [];
  send(msg: Uint8Array) {
    const cmd = msg[5]!;
    const seq = msg[6]!;
    const body = decode8in7(msg.subarray(7, msg.length - 1));
    assert.equal(cmd, SERVICE_CMD.LOGCFG);
    this.bodies.push([...body]);
    if (!this.cardLog) return void this.queue.push(reply(cmd, seq, -60));
    if (body[0] === 3) this.on = body[1]!;
    // on, file b, state, 0, writes 12, last_status -5, ms since write 3000
    this.queue.push(
      reply(cmd, seq, 0, [this.on, 1, this.on ? 1 : 0, 0, 0, 0, 0, 12, 255, 255, 255, 251, 0, 0, 0x0b, 0xb8]),
    );
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

test('cardLog reads op 2 and setCardLog sends op 3; an error status means no card log', async () => {
  const dev = new FakeRuntime();
  const s = new DkSession(dev, { timeoutMs: 200 });
  assert.deepEqual(await s.cardLog(), {
    on: false,
    file: 'b',
    state: 'off',
    writes: 12,
    lastStatus: -5,
    msSinceWrite: 3000,
  });
  assert.deepEqual(dev.bodies.at(-1), [2]);
  const set = await s.setCardLog(true);
  assert.deepEqual(dev.bodies.at(-1), [3, 1]);
  assert.equal(set.on, true);
  assert.equal(set.state, 'on');
  dev.cardLog = false;
  assert.equal(await s.cardLog(), null);
});

test('parseCardLog decodes the header and the valid events of a 16448 byte file', () => {
  const buf = new Uint8Array(64 + 512 * 32);
  const v = new DataView(buf.buffer);
  v.setUint32(0, 0x444b434c);
  v.setUint16(4, 1);
  v.setUint16(6, 64);
  v.setUint16(8, 32);
  v.setUint16(10, 2);
  v.setUint32(12, 7);
  buf[16] = 1;
  v.setUint32(20, 10);
  v.setUint32(24, 11);
  v.setUint32(28, 3000);
  v.setUint32(32, 512);
  v.setUint32(36, 4);
  for (const [i, seq] of [10, 11].entries()) {
    const at = 64 + 32 * i;
    v.setUint32(at, seq);
    v.setUint32(at + 4, seq * 150);
    v.setUint16(at + 8, LOG_KIND.MOD_NEEDS);
    v.setUint16(at + 10, 4);
    v.setUint32(at + 12, seq);
    v.setUint32(at + 28, 0xdead0000 + seq);
  }
  const f = parseCardLog(buf);
  assert.equal(f.file, 'b');
  assert.equal(f.writeNo, 7);
  assert.equal(f.bootSeq, 4);
  assert.equal(f.kind, 'rolling');
  assert.equal(f.newestSeq, 11);
  assert.equal(f.events.length, 2);
  assert.deepEqual(f.events[1], {
    seq: 11,
    time: 1650,
    kind: LOG_KIND.MOD_NEEDS,
    flags: 4,
    args: [11, 0, 0, 0, 0xdead000b],
  });
  buf[17] = 1;
  assert.equal(parseCardLog(buf).kind, 'incident');
  buf[17] = 0;
  assert.throws(() => parseCardLog(new Uint8Array(buf.length)));
  v.setUint16(8, 64); // a wrong event_bytes
  assert.throws(
    () => parseCardLog(buf),
    (e) => e instanceof DkError,
  );
});

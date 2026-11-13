// The card log: LOGCFG op 2 and 3 against a fake debug module, the file decoder on a hand-built file, and the USB_SW event text.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import {
  DkError,
  DkSession,
  decode8in7,
  encode8in7,
  parseCardLog,
  runtimeEvent,
  eventLine,
  summarizeCardLog,
  switchLine,
  type CardLogFile,
  type DkLogEvent,
} from '@digi/dk-session';
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
    v.setUint16(at + 8, LOG_KIND.USB_SW);
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
    kind: LOG_KIND.USB_SW,
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

test('a USB_SW event reads as phase, direction, switch number and register bits', () => {
  const phase = 11 | (2 << 8) | (1 << 12) | (1 << 13) | (5 << 16); // CONFIGURED, port 2, release, detail 1, switch 5
  const sts = (1 << 6) | (1 << 16) | (0x2a << 24); // URI, RS, ULPI 0x2a
  const otg = 1 | 4 | 8 | (3 << 8) | (1 << 16) | (1 << 21) | (9 << 24); // BSV, audio, addr 3, EP1RX, EP3TX, 9 connects
  const d = runtimeEvent({ seq: 1, time: 0, kind: LOG_KIND.USB_SW, flags: 4, args: [phase, 120, sts, 0x1005, otg] });
  assert.deepEqual(d.fields, {
    phase: 'CONFIGURED',
    dir: 'release',
    port: 2,
    detail: 1,
    switch_no: 5,
    ms: 120,
    rs: 1,
    uri: 1,
    pci: 0,
    sli: 0,
    ulpi: 0x2a,
    portsc: '0x00001005',
    bsv: 1,
    sof: 0,
    audio: 1,
    clk: 1,
    addr: 3,
    eps: 'EP1RX,EP3TX',
    connects: 9,
  });
  assert.match(eventLine(d), /USB_SW/);
  const a = runtimeEvent({ seq: 2, time: 0, kind: LOG_KIND.USB_SW, flags: 4, args: [10 | (2 << 13), 0, 0, 0, 0] });
  assert.equal(a.fields.reset, 'uri');
});

test('a PHY_BUSY event shows the reason, the tries and the ULPI viewport bits', () => {
  const phase = 19 | (2 << 8) | (3 << 16); // PHY_BUSY, port 2, take-over, switch 3
  const sts = 4 | (2 << 8) | (1 << 16); // why 4, 2 tries, RS
  const vp = (1 << 30) | (1 << 27) | (2 << 24) | (0x04 << 16) | 0x1234; // RUN, SS, port 2, addr 0x04, data 0x1234
  const d = runtimeEvent({ seq: 1, time: 0, kind: LOG_KIND.USB_SW, flags: 4, args: [phase, 50, sts, vp, 0] });
  assert.equal(d.fields.phase, 'PHY_BUSY');
  assert.equal(d.fields.why, 4);
  assert.equal(d.fields.tries, 2);
  assert.equal(d.fields.rs, 1);
  assert.equal(d.fields.viewport, '0x4a041234');
  assert.deepEqual([d.fields.run, d.fields.rw, d.fields.ss, d.fields.ulpiwu], [1, 0, 1, 0]);
  assert.deepEqual([d.fields.vport, d.fields.vaddr, d.fields.vdata], [2, '0x04', '0x1234']);
});

test('a PHY_WAKE event shows the result and the viewport bits', () => {
  const phase = 21 | (1 << 13) | (4 << 16); // PHY_WAKE, detail 1 (timeout), switch 4
  const d = runtimeEvent({ seq: 1, time: 0, kind: LOG_KIND.USB_SW, flags: 4, args: [phase, 3000, 0x80000000, 15, 0] });
  assert.equal(d.fields.phase, 'PHY_WAKE');
  assert.equal(d.fields.result, 'timeout');
  assert.equal(d.fields.viewport, '0x80000000');
  assert.deepEqual([d.fields.wu, d.fields.run, d.fields.polled_ms, d.fields.clk], [1, 0, 15, 0]);
  assert.equal(d.fields.portsc, undefined);
});

test('summarizeCardLog gives one row per switch and counts overlapping events once', () => {
  let seq = 0;
  const ev = (sw: number, phase: number, detail = 0, ms = 0, portsc = 0, audio = 0, clk = 0): DkLogEvent => ({
    seq: ++seq,
    time: seq,
    kind: LOG_KIND.USB_SW,
    flags: 4,
    args: [phase | (2 << 8) | (detail << 13) | (sw << 16), ms, 0, portsc, (audio << 2) | (clk << 3)],
  });
  const [REQ, PARK, RS_UP, ADDRESSED, ENUM_LATE, RECOVERY, GIVEUP, DONE, WAKE] = [1, 6, 8, 10, 13, 14, 15, 16, 21];
  const events = [
    ev(1, REQ),
    ev(1, PARK, 0),
    ev(1, RS_UP, 0, 2066, 0xa0000805),
    ev(1, ADDRESSED, 3, 2200),
    ev(1, DONE, 0, 2210),
    ev(2, REQ, 0, 0, 0, 1),
    ev(2, PARK, 1),
    ev(2, RS_UP, 0, 2066, 0xa0000005),
    ev(2, ENUM_LATE, 0, 5066),
    ev(2, RECOVERY, 1, 5100),
    ev(2, ADDRESSED, 1, 8000),
    ev(2, DONE, 0, 8010),
    ev(3, REQ),
    ev(3, RS_UP, 0, 2000, 0xa0000005, 0, 1),
    ev(3, WAKE, 1, 4000),
    ev(3, ENUM_LATE, 0, 5000),
    ev(3, GIVEUP, 0, 9000),
  ];
  const file = (kind: 'rolling' | 'incident', evs: DkLogEvent[]) =>
    ({ bootSeq: 5, kind, file: 'a', events: evs }) as unknown as CardLogFile;
  // the incident file repeats the last ten events of the rolling file
  const s = summarizeCardLog([file('rolling', events), file('incident', events.slice(6))]);
  assert.equal(s.length, 3);
  assert.deepEqual(
    s.map((x) => [x.switchNo, x.park, x.lineState, x.doneMs, x.stoppedAt, x.enumLate, x.recoveries, x.selfHealed]),
    [
      [1, 'ok', 'J', 2210, null, false, 0, false],
      [2, 'refused', 'SE0', 8010, null, true, 1, true],
      [3, '-', 'SE0', null, 'GIVEUP', true, 0, false],
    ],
  );
  assert.equal(s[0]!.reset, 'addr0');
  assert.equal(s[1]!.audio, true);
  assert.equal(s[2]!.giveUp, true);
  assert.deepEqual([s[2]!.clk, s[2]!.clkEnd, s[2]!.wake], [1, 0, 'timeout']);
  assert.match(switchLine(s[2]!), /clk=1 .*clk_end=0 .*wake=timeout/);
});
